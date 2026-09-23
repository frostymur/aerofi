use crate::core::item::{ScriptMode, Target};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const BOUNDARY: &str = "__AEROFI_BOUNDARY__";

/// One byte written to a daemon's stdin re-sources its script immediately
/// (the wrapper's `read` returns early). Pacing the writes paces the ticks.
const WAKE: &[u8] = b"\n";

/// Safety-net timeout for the daemon wrapper's `read`: if aerofi's pacing
/// ever stops (or the window stays hidden for a long time), the daemon
/// still refreshes at least this often.
const DAEMON_FALLBACK_SECS: u64 = 3600;

/// Parses a refresh time string like "5m", "1h", "30s" into a Duration.
pub fn parse_refresh_time(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, unit) = s.split_at(s.len().saturating_sub(1));
    if let Ok(num) = num_str.parse::<u64>() {
        match unit {
            "s" => Some(Duration::from_secs(num)),
            "m" => Some(Duration::from_secs(num * 60)),
            "h" => Some(Duration::from_secs(num * 3600)),
            "d" => Some(Duration::from_secs(num * 86400)),
            _ => None,
        }
    } else {
        None
    }
}

/// One running inline-script daemon.
struct DaemonInfo {
    /// The daemon's bash pid (== its process group id, see
    /// `executor::with_own_process_group`).
    pid: u32,
    /// Script the daemon sources (a path change restarts the daemon).
    path: PathBuf,
    /// Refresh interval in seconds (a change restarts the daemon).
    secs: u64,
    /// The daemon's stdin — writing to it paces the daemon's refresh ticks
    /// (see [`WAKE`]).
    stdin: Arc<Mutex<Option<ChildStdin>>>,
}

/// When each daemon last received a pacing wake. Drives the per-daemon
/// cadence: a daemon is woken once its interval has elapsed.
static LAST_TICK: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Running daemons, keyed by script filename stem.
static DAEMONS: LazyLock<Mutex<HashMap<String, DaemonInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Events flowing from the daemon reader threads (and the main thread)
/// into the single dispatcher task.
enum DaemonEvent {
    /// A daemon produced new output.
    Output(PathBuf, String),
    /// The window was shown: poke every daemon for a fresh tick and resume
    /// pacing.
    Show,
    /// The window was hidden: stop pacing.
    Hide,
}

/// Event channel shared by every daemon thread. One app-scoped dispatcher
/// task (started by `start_daemon`) applies messages to the launcher and
/// paces the daemons' refresh cadence; the global sender keeps that task
/// alive for the whole app run.
type DaemonEventTx = futures::channel::mpsc::UnboundedSender<DaemonEvent>;

static OUTPUT_TX: LazyLock<Mutex<Option<DaemonEventTx>>> = LazyLock::new(|| Mutex::new(None));

/// Tell the dispatcher about a window visibility change so it resumes or
/// stops pacing the daemons. Call from the main thread.
pub fn notify_visibility(visible: bool) {
    let Some(tx) = OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    else {
        return;
    };
    let _ = tx.unbounded_send(if visible {
        DaemonEvent::Show
    } else {
        DaemonEvent::Hide
    });
}

/// Latest per-daemon output produced while the window was hidden.
///
/// Pushing a hidden tick over the channel would wake the main thread (and
/// run a `view.update`) for output nobody is looking at. Instead the reader
/// thread parks the tick here, and [`flush_pending_inline`] drains it when
/// the window is shown again. Keyed by script path, so at most one entry
/// per inline daemon is retained.
static PENDING_INLINE: LazyLock<Mutex<HashMap<PathBuf, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Drain and return the inline outputs buffered while the window was
/// hidden. Must be called on the main thread (e.g. from `on_show`); the
/// caller is expected to apply each entry to the launcher.
pub fn flush_pending_inline() -> Vec<(PathBuf, String)> {
    let mut pending = PENDING_INLINE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    pending.drain().collect()
}

fn lock_daemons() -> std::sync::MutexGuard<'static, HashMap<String, DaemonInfo>> {
    DAEMONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Remove a daemon from the registry, but only if the stored entry still
/// belongs to the given pid (a reload may have replaced it meanwhile).
fn daemon_remove(stem: &str, pid: u32) {
    let mut daemons = lock_daemons();
    if daemons.get(stem).is_some_and(|d| d.pid == pid) {
        daemons.remove(stem);
    }
}

/// The daemon set the given targets require: every inline script with a
/// refresh time, deduplicated by filename stem (first occurrence wins).
fn desired_daemons(targets: &[Target]) -> HashMap<String, (PathBuf, u64)> {
    let mut desired = HashMap::new();
    for target in targets {
        if let Target::Script { mode, path, .. } = target
            && *mode == ScriptMode::Inline
            && let Some(refresh_str) = target.refresh_time()
            && let Some(duration) = parse_refresh_time(refresh_str)
        {
            let stem = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            desired
                .entry(stem)
                .or_insert_with(|| (path.to_path_buf(), duration.as_secs().max(1)));
        }
    }
    desired
}

/// Apply one daemon event to the launcher, returning the resulting
/// visibility state: `Show`/`Hide` override it, `Output` preserves it.
/// Kept as a plain (synchronous) function so the dispatcher's borrow of
/// `visible` never spans an await point.
fn handle_daemon_event(
    ev: DaemonEvent,
    visible: bool,
    app: &gpui::AsyncApp,
    view: &gpui::Entity<crate::ui::launcher::Launcher>,
) -> bool {
    match ev {
        DaemonEvent::Output(path, text) => {
            app.update(|cx| {
                view.update(cx, |launcher, cx| {
                    launcher.apply_inline_output(&path, Some(gpui::SharedString::from(text)));
                    if crate::ui::window::is_visible() {
                        cx.notify();
                    }
                });
            });
            visible
        }
        DaemonEvent::Show => {
            // Fresh data on the first frame after a show.
            poke_all_daemons();
            true
        }
        DaemonEvent::Hide => false,
    }
}

/// The time until the next daemon's pacing wake is due, or an hour if
/// there are no daemons (the timer then just re-checks state).
fn next_pace_in() -> Duration {
    let now = Instant::now();
    let daemons = lock_daemons();
    let last = LAST_TICK.lock().unwrap_or_else(|p| p.into_inner());
    daemons
        .iter()
        .map(|(stem, d)| {
            let last_at = last.get(stem).copied().unwrap_or(now);
            (last_at + Duration::from_secs(d.secs)).saturating_duration_since(now)
        })
        .min()
        .unwrap_or(Duration::from_secs(DAEMON_FALLBACK_SECS))
}

/// Write a pacing wake to every daemon whose refresh interval has elapsed.
/// Called by the dispatcher's timer while the window is visible.
fn pace_daemons() {
    let now = Instant::now();
    let daemons = lock_daemons();
    let mut last = LAST_TICK.lock().unwrap_or_else(|p| p.into_inner());
    for (stem, d) in daemons.iter() {
        let due = last
            .get(stem)
            .is_none_or(|t| now.duration_since(*t) >= Duration::from_secs(d.secs));
        if due {
            let _ = d
                .stdin
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
                .and_then(|s| s.write_all(WAKE).ok());
            last.insert(stem.clone(), now);
        }
    }
}

/// Immediately re-source every daemon by writing a wake to its stdin.
/// Called when the window is shown so inline subtitles are fresh on the
/// first frame. Must be called on the main thread.
pub fn poke_all_daemons() {
    let now = Instant::now();
    let daemons = lock_daemons();
    let mut last = LAST_TICK.lock().unwrap_or_else(|p| p.into_inner());
    for (stem, d) in daemons.iter() {
        let _ = d
            .stdin
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
            .and_then(|s| s.write_all(WAKE).ok());
        last.insert(stem.clone(), now);
    }
}

/// Start the inline-script daemon subsystem: installs the shared output
/// channel, spawns the single dispatcher task that pushes daemon output to
/// the launcher, and starts the daemons for `all_targets`.
///
/// Uses only std::thread + a foreground-executor task — no smol thread
/// pool (initializing it would cost ~8 threads × 2 MB).
pub fn start_daemon(
    cx: &mut gpui::App,
    all_targets: &[Target],
    view: gpui::Entity<crate::ui::launcher::Launcher>,
) {
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<DaemonEvent>();
    *OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);

    // Single dispatcher for all daemons: applies daemon output as it
    // arrives and — while the window is visible — writes the pacing wakes
    // to the daemons' stdin. While hidden it parks on the event channel
    // with no timer at all: the `Show` event wakes it immediately, so the
    // daemons (and their reader threads) stay fully asleep in the
    // background.
    let app_async = cx.to_async();
    let dispatcher_app = app_async.clone();
    // Owned handle (cloned out of `app_async`) so the timer futures the
    // dispatcher awaits are 'static and the async block never borrows the
    // spawn closure's `&mut AsyncApp` parameter.
    let bg = app_async.background_executor().clone();
    app_async
        .spawn(move |_: &mut gpui::AsyncApp| async move {
            use futures::{FutureExt, StreamExt};
            let mut visible = crate::ui::window::is_visible();
            loop {
                if visible {
                    let delay = next_pace_in();
                    futures::select! {
                        ev = rx.next() => {
                            let Some(ev) = ev else { return };
                            visible = handle_daemon_event(ev, visible, &dispatcher_app, &view);
                        }
                        _ = bg.timer(delay).fuse() => {
                            pace_daemons();
                        }
                    }
                } else {
                    // Hidden: park on the channel with no timer at all. A
                    // `Show` event wakes this immediately and resumes pacing.
                    let Some(ev) = rx.next().await else {
                        return;
                    };
                    visible = handle_daemon_event(ev, visible, &dispatcher_app, &view);
                }
            }
        })
        .detach();

    reconcile_daemons(all_targets);
}

/// Align the running daemons with `targets`: kill daemons whose script
/// disappeared or whose path/refresh time changed, start missing ones.
/// Called from `start_daemon` (startup) and `Launcher::reload` (Cmd+R).
pub fn reconcile_daemons(targets: &[Target]) {
    let desired = desired_daemons(targets);
    let mut daemons = lock_daemons();

    // Kill stale daemons (script removed, moved, or interval changed).
    let stale: Vec<(String, u32)> = daemons
        .iter()
        .filter(|(stem, info)| {
            !desired
                .get(stem.as_str())
                .is_some_and(|(path, secs)| path == &info.path && *secs == info.secs)
        })
        .map(|(stem, info)| (stem.clone(), info.pid))
        .collect();
    for (stem, pid) in stale {
        daemons.remove(&stem);
        // Group kill: the daemon bash plus everything the sourced script
        // spawned. The daemon thread reaps its child on exit.
        unsafe { libc::kill(-(pid as libc::c_int), libc::SIGKILL) };
        crate::core::executor::unregister_script(pid);
    }

    // Start missing daemons.
    for (stem, (path, secs)) in desired.iter() {
        if daemons.get(stem).is_some() {
            continue;
        }
        let Some((pid, stdin)) = spawn_daemon(stem, path) else {
            continue;
        };
        daemons.insert(
            stem.clone(),
            DaemonInfo {
                pid,
                path: path.clone(),
                secs: *secs,
                stdin,
            },
        );
    }
}

/// Spawn one daemon: a long-lived bash wrapper that sources the script,
/// emits its output delimited by a boundary line, and then waits for aerofi
/// to write a wake byte to its stdin (pacing the refresh). A long `read -t`
/// timeout is the safety net in case aerofi never paces (e.g. the window
/// stays hidden for a long time).
///
/// Returns the bash pid (registered for quit-time kill) and the handle to
/// the daemon's stdin, used to send the pacing wakes.
fn spawn_daemon(stem: &str, path: &Path) -> Option<(u32, Arc<Mutex<Option<ChildStdin>>>)> {
    let Some(tx) = OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    else {
        eprintln!("aerofi: scheduler: no dispatcher yet, skipping daemon for {stem}");
        return None;
    };

    // A fresh daemon starts its own cadence.
    LAST_TICK
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(stem);

    let path_str = path.to_string_lossy().into_owned();
    // The sourced script's stdin is /dev/null so it can never consume a
    // pacing wake; only the outer `read` reads from the pipe.
    let bash_script = format!(
        "while true; do\n  . \"{script}\" < /dev/null 2>/dev/null || true\n  printf '%s\\n' '{boundary}'\n  read -t {fallback} _ 2>/dev/null || true\ndone",
        script = path_str,
        boundary = BOUNDARY,
        fallback = DAEMON_FALLBACK_SECS,
    );

    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg(&bash_script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Own process group so quit-time group kills reach the daemon bash and
    // its children.
    crate::core::executor::with_own_process_group(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aerofi: scheduler: failed to spawn bash for {path_str}: {e}");
            return None;
        }
    };
    let pid = child.id();
    // Track for quit-time kill (atexit `kill_all_scripts`).
    crate::core::executor::register_script(pid);
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            crate::core::executor::unregister_script(pid);
            return None;
        }
    };
    let stdin = child.stdin.take();

    let path2 = path.to_path_buf();
    let stem2 = stem.to_string();
    // macOS default thread stack = 8 MB. Our loop only does poll + read +
    // channel send — 256 KB is plenty.
    let _ = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .name(format!("aerofi-inline:{}", path_str))
        .spawn(move || {
            // Read the daemon's stdout with poll() rather than blocking on a
            // BufReader, so the loop can also observe the wrapper bash
            // exiting. Relying on pipe EOF alone would hang this thread if a
            // background grandchild the script spawned keeps the pipe's write
            // end open after the wrapper is killed — leaking the wrapper as a
            // zombie (never reaped), this thread, and the registry entries.
            let fd = stdout.as_raw_fd();
            let mut read_buf = [0u8; 8192];
            let mut line_acc: Vec<u8> = Vec::new();
            let mut buf: Vec<String> = Vec::new();
            let mut reaped = false;

            // Handle one complete line from the daemon stream. Returns `false`
            // to stop reading (the dispatcher is gone).
            let mut handle_line = |line: &str| -> bool {
                if line.trim() == BOUNDARY {
                    let text = buf.join("\n");
                    let text = text.trim().to_string();
                    buf.clear();

                    if !text.is_empty() {
                        if crate::ui::window::is_visible() {
                            // UnboundedSender::unbounded_send never blocks.
                            if tx
                                .unbounded_send(DaemonEvent::Output(path2.clone(), text))
                                .is_err()
                            {
                                // Dispatcher gone — UI shut down.
                                return false;
                            }
                        } else {
                            // Hidden: buffer the latest tick without waking
                            // the main thread; `Launcher::on_show` flushes
                            // it via `flush_pending_inline`.
                            let mut pending = PENDING_INLINE
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            pending.insert(path2.clone(), text);
                        }
                    }
                } else if buf.len() < 500 {
                    buf.push(line.to_string());
                }
                true
            };

            'read: loop {
                // The wrapper exited: stop and reap it even if a grandchild
                // still holds the pipe open.
                if let Ok(Some(_)) = child.try_wait() {
                    reaped = true;
                    break;
                }

                let mut pollfd = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let rc = unsafe { libc::poll(&mut pollfd, 1, 200) };
                if rc < 0 {
                    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                        continue;
                    }
                    break;
                }
                if rc == 0 {
                    continue; // timeout — loop back to re-check the wrapper
                }

                let revents = pollfd.revents;
                if revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                    break;
                }
                if revents & (libc::POLLIN | libc::POLLHUP) == 0 {
                    continue;
                }

                let mut eof = false;
                loop {
                    let n = unsafe { libc::read(fd, read_buf.as_mut_ptr().cast(), read_buf.len()) };
                    if n > 0 {
                        line_acc.extend_from_slice(&read_buf[..n as usize]);
                        while let Some(pos) = line_acc.iter().position(|&b| b == b'\n') {
                            let mut line_bytes: Vec<u8> = line_acc.drain(..=pos).collect();
                            // Strip the trailing newline (and a preceding \r)
                            // to match BufReader::lines().
                            if line_bytes.last() == Some(&b'\n') {
                                line_bytes.pop();
                                if line_bytes.last() == Some(&b'\r') {
                                    line_bytes.pop();
                                }
                            }
                            let line = String::from_utf8_lossy(&line_bytes).into_owned();
                            if !handle_line(&line) {
                                break 'read;
                            }
                        }
                    } else if n == 0 {
                        eof = true;
                        break;
                    } else {
                        let e = std::io::Error::last_os_error();
                        if e.raw_os_error() == Some(libc::EINTR) {
                            continue; // interrupted — retry the read
                        }
                        break;
                    }
                }
                if eof {
                    break;
                }
            }

            // Flush any trailing line that didn't end in a newline.
            if !line_acc.is_empty() {
                let line = String::from_utf8_lossy(&line_acc).into_owned();
                let _ = handle_line(&line);
            }

            // Exit cleanup: kill (no-op if already dead, e.g. a reload
            // restarted us) and reap the child if not already reaped via
            // try_wait, then drop the registry entries if they still point
            // at us.
            if !reaped {
                let _ = child.kill();
                let _ = child.wait();
            }
            crate::core::executor::unregister_script(pid);
            daemon_remove(&stem2, pid);
        });

    Some((pid, Arc::new(Mutex::new(stdin))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_pending_inline_drains_latest_per_path() {
        let path = PathBuf::from("/tmp/aerofi-test-inline.sh");
        // Simulate two hidden ticks for the same daemon (latest wins).
        for text in ["stale", "fresh"] {
            PENDING_INLINE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(path.clone(), text.to_string());
        }

        let out = flush_pending_inline();
        assert_eq!(out, vec![(path.clone(), "fresh".to_string())]);

        // A second flush is empty (buffer was drained).
        assert!(flush_pending_inline().is_empty());
    }
}
