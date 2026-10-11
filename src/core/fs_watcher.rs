//! Live source watching: rescan application, script, and plugin
//! directories on filesystem changes so new/removed items appear without a
//! manual reload (Cmd+R).
//!
//! The watch set is derived from the config specs, not hard-coded:
//! apps get the standard macOS bundle dirs plus `[apps] extra_dirs` /
//! `extra_apps` (minus `ignore_dirs`), scripts get `[scripts] dirs`, and
//! plugins get the two plugin directories. A source disabled in `[sources]`
//! contributes no watched dirs at all. [`reconcile`] re-derives the set
//! after every config reload.
//!
//! Threading model mirrors `scheduler`: one std thread (the `notify`
//! FSEvents runner) plus a single app-scoped dispatcher task on the
//! foreground executor — no smol thread pool (initializing it costs
//! ~8 threads × 2 MB). All three source kinds share one FSEvents stream
//! and one dispatcher task; watching more paths costs nothing extra.
//!
//! Changes that arrive while the window is hidden never wake the main
//! thread: the event callback parks them in per-kind [`PENDING`] flags
//! instead, and `Launcher::on_show` drains them via [`take_pending`]
//! before the first frame (the same scheme as the scheduler's
//! `PENDING_INLINE`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Debounce window: a save burst (editor tmp-file + rename, chmod, an
/// installer dropping a bundle, ...) coalesces into one rescan per kind.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Idle timer when there is no pending change (re-checks state only),
/// mirroring the daemon pacing's fallback scheme.
const IDLE: Duration = Duration::from_secs(3600);

/// Which source kind a change belongs to. The order is the index used by
/// the `PENDING` flags and the dispatcher's dirty bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Apps = 0,
    Scripts = 1,
    Plugins = 2,
}

impl SourceKind {
    pub const ALL: [Self; 3] = [Self::Apps, Self::Scripts, Self::Plugins];

    fn index(self) -> usize {
        self as usize
    }
}

/// Events flowing into the dispatcher task from the notify callback and
/// the main-thread visibility hooks.
enum Event {
    /// A watched folder for this source kind changed.
    Change(SourceKind),
    /// The window was shown.
    Show,
    /// The window was hidden.
    Hide,
}

type EventTx = futures::channel::mpsc::UnboundedSender<Event>;

static OUTPUT_TX: LazyLock<Mutex<Option<EventTx>>> = LazyLock::new(|| Mutex::new(None));
static WATCHER: LazyLock<Mutex<Option<RecommendedWatcher>>> = LazyLock::new(|| Mutex::new(None));
/// Currently watched paths and the kind each one belongs to. Read by the
/// notify callback to classify events; written by `set_watched` on the
/// main thread. Tiny (a dozen paths), so a plain mutex is fine.
static KIND_DIRS: LazyLock<Mutex<Vec<(PathBuf, SourceKind)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// "A folder changed while the window was hidden" flags, one per
/// [`SourceKind`]. Set by the notify callback (non-main thread), drained
/// by `Launcher::on_show` (main thread).
static PENDING: [AtomicBool; 3] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

/// Drain the pending-rescan flags. Returns `(apps, scripts, plugins)`.
/// Must be called on the main thread (e.g. from `on_show`).
pub fn take_pending() -> (bool, bool, bool) {
    (
        PENDING[SourceKind::Apps.index()].swap(false, Ordering::Relaxed),
        PENDING[SourceKind::Scripts.index()].swap(false, Ordering::Relaxed),
        PENDING[SourceKind::Plugins.index()].swap(false, Ordering::Relaxed),
    )
}

/// Tell the dispatcher about a window visibility change so it resumes
/// (Show) or parks (Hide). Call from the main thread.
pub fn notify_visibility(visible: bool) {
    let Some(tx) = OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    else {
        return;
    };
    let _ = tx.unbounded_send(if visible { Event::Show } else { Event::Hide });
}

/// The watched paths for the given source kind, per the config specs:
/// apps (standard bundle dirs + `[apps] extra_dirs`/`extra_apps`),
/// scripts (`[scripts] dirs`), plugins (the plugin directories). A source
/// disabled in `[sources]` yields nothing. Missing paths are still
/// returned: `set_watched` skips them with a warning, matching the
/// scanners' behavior.
pub fn watch_dirs_for(kind: SourceKind, config: &crate::core::config::AppConfig) -> Vec<PathBuf> {
    match kind {
        SourceKind::Apps => crate::core::scanner::app_watch_dirs(config),
        SourceKind::Scripts => {
            if !config.sources.scripts {
                return Vec::new();
            }
            config.expanded_script_dirs()
        }
        SourceKind::Plugins => crate::core::plugin_manager::PluginManager::plugin_dirs(),
    }
}

/// Re-point the watcher at the configured folders after a config reload
/// (`scripts.dirs`, `[apps]`, or `[sources]` may have changed). A no-op
/// when the watcher was never started (e.g. `watch_sources = false`).
pub fn reconcile(config: &crate::core::config::AppConfig) {
    let dirs = all_watch_dirs(config);
    let mut guard = WATCHER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(watcher) = guard.as_mut() else {
        return;
    };
    set_watched(watcher, &dirs);
}

/// Every (dir, kind) pair to watch for the given config.
fn all_watch_dirs(config: &crate::core::config::AppConfig) -> Vec<(PathBuf, SourceKind)> {
    let mut dirs = Vec::new();
    for kind in SourceKind::ALL {
        for dir in watch_dirs_for(kind, config) {
            dirs.push((dir, kind));
        }
    }
    dirs
}

/// Classify event paths into the source kinds they touch, using the
/// current watch set. FSEvents delivers paths of files *inside* a watched
/// dir, so classification is by prefix.
fn classify(paths: &[PathBuf]) -> Vec<SourceKind> {
    let guard = KIND_DIRS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut kinds = Vec::new();
    for path in paths {
        for (dir, kind) in guard.iter() {
            if (path == dir || path.starts_with(dir)) && !kinds.contains(kind) {
                kinds.push(*kind);
            }
        }
    }
    kinds
}

/// Start the source watcher and its dispatcher task. Called once from
/// `main` after the launcher window exists (like `start_daemon`).
pub fn start(
    cx: &mut gpui::App,
    config: &crate::core::config::AppConfig,
    // Called on the main thread (via `app.update`) for each dirty source
    // kind; the UI closure applies the rescan and notifies the view.
    mut apply_change: impl FnMut(SourceKind, &mut gpui::App) + Send + 'static,
) {
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<Event>();
    *OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);

    // The callback runs on the notify event thread. A change while the
    // window is hidden must not wake the main thread: park it in the
    // per-kind atomic flags instead; `on_show` applies them before the
    // first frame.
    let callback = move |res: Result<notify::Event, notify::Error>| match res {
        Ok(event) => {
            let kinds = classify(&event.paths);
            if kinds.is_empty() {
                return;
            }
            if crate::sys::appkit::window_visible() {
                let Some(tx) = OUTPUT_TX
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
                else {
                    return;
                };
                // UnboundedSender::unbounded_send never blocks.
                for kind in kinds {
                    let _ = tx.unbounded_send(Event::Change(kind));
                }
            } else {
                for kind in kinds {
                    PENDING[kind.index()].store(true, Ordering::Relaxed);
                }
            }
        }
        Err(e) => eprintln!("aerofi: fs watcher: {e}"),
    };

    let mut watcher = match notify::recommended_watcher(callback) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("aerofi: failed to start fs watcher: {e}");
            return;
        }
    };
    let dirs = all_watch_dirs(config);
    set_watched(&mut watcher, &dirs);
    *WATCHER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(watcher);

    // Single dispatcher: coalesces change bursts into debounced rescans
    // (one per dirty kind) while the window is visible, parks on the
    // channel while it is hidden (a `Show` event wakes it immediately).
    let app_async = cx.to_async();
    let dispatcher_app = app_async.clone();
    // Owned handle (cloned out of `app_async`) so the timer futures the
    // dispatcher awaits are 'static and the async block never borrows
    // the spawn closure's `&mut AsyncApp` parameter.
    let bg = app_async.background_executor().clone();
    app_async
        .spawn(move |_: &mut gpui::AsyncApp| async move {
            use futures::{FutureExt, StreamExt};
            let mut visible = crate::sys::appkit::window_visible();
            // Per-kind dirty bits, coalesced by one shared timer.
            let mut dirty = [false; 3];
            let mut next = IDLE;
            loop {
                if visible {
                    futures::select! {
                        ev = rx.next() => {
                            let Some(ev) = ev else { return };
                            match ev {
                                Event::Change(kind) => {
                                    dirty[kind.index()] = true;
                                    next = DEBOUNCE;
                                }
                                // A Show while already visible is a
                                // no-op: the debounced rescan (if any)
                                // stays armed.
                                Event::Show => {}
                                Event::Hide => {
                                    visible = false;
                                }
                            }
                        }
                        _ = bg.timer(next).fuse() => {
                            for kind in SourceKind::ALL {
                                if dirty[kind.index()] {
                                    dirty[kind.index()] = false;
                                    rescan(&mut apply_change, &dispatcher_app, kind);
                                }
                            }
                            next = IDLE;
                        }
                    }
                } else {
                    // Hidden: park on the channel. The callback only
                    // sends Show/Hide while hidden, so no change ever
                    // wakes us; pending changes are drained by on_show.
                    let Some(ev) = rx.next().await else {
                        return;
                    };
                    match ev {
                        Event::Change(kind) => {
                            visible = true;
                            dirty[kind.index()] = true;
                            next = DEBOUNCE;
                        }
                        Event::Show => visible = true,
                        Event::Hide => {}
                    }
                }
            }
        })
        .detach();
}

/// Apply a rescan for one source kind to the launcher. The rescans are
/// cheap (a couple of `read_dir` passes; icon extraction is disk-cached
/// and plugin loading is path-cached); the notify is gated on visibility
/// so a race with a hide never renders a frame nobody looks at.
fn rescan(
    apply_change: &mut dyn FnMut(SourceKind, &mut gpui::App),
    app: &gpui::AsyncApp,
    kind: SourceKind,
) {
    app.update(|cx| apply_change(kind, cx));
}

/// (Re)watch the given (dir, kind) folders: drop paths that are no longer
/// configured and add new ones (missing paths are skipped with a warning,
/// matching the scanners).
fn set_watched(watcher: &mut RecommendedWatcher, dirs: &[(PathBuf, SourceKind)]) {
    let mut guard = KIND_DIRS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current: Vec<(PathBuf, SourceKind)> = guard.clone();
    for (old, kind) in &current {
        if !dirs.contains(&(old.clone(), *kind))
            && let Err(e) = watcher.unwatch(old)
        {
            eprintln!("aerofi: fs watcher: unwatch {}: {e}", old.display());
        }
    }
    // Collect first: the filter closures borrow `guard`, which the `push`
    // below mutates.
    let to_add: Vec<&(PathBuf, SourceKind)> = dirs
        .iter()
        .filter(|(dir, kind)| !current.contains(&(dir.clone(), *kind)))
        .collect();
    for (dir, kind) in to_add {
        match watcher.watch(dir, RecursiveMode::NonRecursive) {
            Ok(()) => guard.push((dir.clone(), *kind)),
            Err(e) => {
                eprintln!("aerofi: fs watcher: cannot watch {}: {e}", dir.display());
            }
        }
    }
}
