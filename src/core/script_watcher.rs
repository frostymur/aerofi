//! Live script-folder watching: rescan `[scripts] dirs` on filesystem
//! changes so new, removed, or modified scripts appear without a manual
//! reload (Cmd+R).
//!
//! Threading model mirrors `scheduler`: one std thread (the `notify`
//! FSEvents runner) plus a single app-scoped dispatcher task on the
//! foreground executor — no smol thread pool (initializing it costs
//! ~8 threads × 2 MB).
//!
//! Changes that arrive while the window is hidden never wake the main
//! thread: the event callback parks them in [`PENDING_RESCAN`] instead,
//! and `Launcher::on_show` drains it via [`take_pending`] before the
//! first frame (the same scheme as the scheduler's `PENDING_INLINE`).

use gpui::Entity;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Debounce window: a save burst (editor tmp-file + rename, chmod, ...)
/// coalesces into a single rescan.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Idle timer when there is no pending change (re-checks state only),
/// mirroring the daemon pacing's fallback scheme.
const IDLE: Duration = Duration::from_secs(3600);

/// Events flowing into the dispatcher task from the notify callback and
/// the main-thread visibility hooks.
enum Event {
    /// A watched folder changed.
    Change,
    /// The window was shown.
    Show,
    /// The window was hidden.
    Hide,
}

type EventTx = futures::channel::mpsc::UnboundedSender<Event>;

static OUTPUT_TX: LazyLock<Mutex<Option<EventTx>>> = LazyLock::new(|| Mutex::new(None));
static WATCHER: LazyLock<Mutex<Option<RecommendedWatcher>>> = LazyLock::new(|| Mutex::new(None));
static WATCHED: LazyLock<Mutex<Vec<PathBuf>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// "A folder changed while the window was hidden" flag. Set by the notify
/// callback (non-main thread), drained by `Launcher::on_show` (main
/// thread).
static PENDING_RESCAN: AtomicBool = AtomicBool::new(false);

/// Drain the pending-rescan flag. Returns true when a rescan is needed.
/// Must be called on the main thread (e.g. from `on_show`).
pub fn take_pending() -> bool {
    PENDING_RESCAN.swap(false, Ordering::Relaxed)
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

/// Re-point the watcher at the configured script folders after a config
/// reload (`scripts.dirs` may have changed). A no-op when the watcher was
/// never started (e.g. `watch_scripts = false`).
pub fn reconcile_dirs(dirs: &[PathBuf]) {
    let mut guard = WATCHER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(watcher) = guard.as_mut() else {
        return;
    };
    set_watched(watcher, dirs);
}

/// Start the script-folder watcher and its dispatcher task. Called once
/// from `main` after the launcher window exists (like `start_daemon`).
pub fn start(cx: &mut gpui::App, dirs: Vec<PathBuf>, view: Entity<crate::ui::launcher::Launcher>) {
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<Event>();
    *OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);

    // The callback runs on the notify event thread. A change while the
    // window is hidden must not wake the main thread: park it in the
    // atomic flag instead; `on_show` applies it before the first frame.
    let callback = move |res: Result<notify::Event, notify::Error>| match res {
        Ok(_) => {
            if crate::ui::window::is_visible() {
                let Some(tx) = OUTPUT_TX
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
                else {
                    return;
                };
                // UnboundedSender::unbounded_send never blocks.
                let _ = tx.unbounded_send(Event::Change);
            } else {
                PENDING_RESCAN.store(true, Ordering::Relaxed);
            }
        }
        Err(e) => eprintln!("aerofi: script watcher: {e}"),
    };

    let mut watcher = match notify::recommended_watcher(callback) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("aerofi: failed to start script watcher: {e}");
            return;
        }
    };
    set_watched(&mut watcher, &dirs);
    *WATCHER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(watcher);

    // Single dispatcher: coalesces change bursts into one debounced
    // rescan while the window is visible, parks on the channel while it
    // is hidden (a `Show` event wakes it immediately).
    let app_async = cx.to_async();
    let dispatcher_app = app_async.clone();
    // Owned handle (cloned out of `app_async`) so the timer futures the
    // dispatcher awaits are 'static and the async block never borrows
    // the spawn closure's `&mut AsyncApp` parameter.
    let bg = app_async.background_executor().clone();
    app_async
        .spawn(move |_: &mut gpui::AsyncApp| async move {
            use futures::{FutureExt, StreamExt};
            let mut visible = crate::ui::window::is_visible();
            let mut dirty = false;
            let mut next = IDLE;
            loop {
                if visible {
                    futures::select! {
                        ev = rx.next() => {
                            let Some(ev) = ev else { return };
                            match ev {
                                Event::Change => {
                                    dirty = true;
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
                            if dirty {
                                dirty = false;
                                rescan(&view, &dispatcher_app);
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
                        Event::Change => {
                            visible = true;
                            dirty = true;
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

/// Apply a rescan to the launcher. The rescan itself is cheap (a couple
/// of `read_dir` passes); the notify is gated on visibility so a
/// race with a hide never renders a frame nobody looks at.
fn rescan(view: &Entity<crate::ui::launcher::Launcher>, app: &gpui::AsyncApp) {
    app.update(|cx| {
        view.update(cx, |launcher, cx| {
            launcher.rescan_scripts();
            if crate::ui::window::is_visible() {
                cx.notify();
            }
        });
    });
}

/// (Re)watch the given folders: drop paths that are no longer configured
/// and add new ones (missing folders are skipped with a warning, matching
/// `scan_scripts`).
fn set_watched(watcher: &mut RecommendedWatcher, dirs: &[PathBuf]) {
    let mut watched = WATCHED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for old in watched.iter().filter(|old| !dirs.contains(old)) {
        if let Err(e) = watcher.unwatch(old) {
            eprintln!("aerofi: script watcher: unwatch {}: {e}", old.display());
        }
    }
    watched.retain(|old| dirs.contains(old));
    // Collect first: the filter closure borrows `watched`, which the
    // `push` below mutates.
    let to_add: Vec<&PathBuf> = dirs.iter().filter(|dir| !watched.contains(dir)).collect();
    for dir in to_add {
        match watcher.watch(dir, RecursiveMode::NonRecursive) {
            Ok(()) => watched.push(dir.clone()),
            Err(e) => {
                eprintln!(
                    "aerofi: script watcher: cannot watch {}: {e}",
                    dir.display()
                );
            }
        }
    }
}
