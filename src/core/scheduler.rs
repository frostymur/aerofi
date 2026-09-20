use crate::core::item::{ScriptMode, Target};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

const BOUNDARY: &str = "__AEROFI_BOUNDARY__";

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
}

/// Running daemons, keyed by script filename stem.
static DAEMONS: LazyLock<Mutex<HashMap<String, DaemonInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Output channel shared by every daemon thread. One app-scoped dispatcher
/// task (started by `start_daemon`) applies messages to the launcher; the
/// global sender keeps that task alive for the whole app run.
type DaemonOutputTx = futures::channel::mpsc::UnboundedSender<(PathBuf, String)>;

static OUTPUT_TX: LazyLock<Mutex<Option<DaemonOutputTx>>> = LazyLock::new(|| Mutex::new(None));

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
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<(PathBuf, String)>();
    *OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);

    // Single dispatcher for all daemons: truly sleeps via Stream::next()
    // until a message arrives — zero CPU between script outputs.
    let app_async = cx.to_async();
    let dispatcher_app = app_async.clone();
    app_async
        .spawn(move |_: &mut gpui::AsyncApp| async move {
            use futures::StreamExt;
            while let Some((path, text)) = rx.next().await {
                dispatcher_app.update(|cx| {
                    view.update(cx, |launcher, cx| {
                        launcher.apply_inline_output(&path, Some(gpui::SharedString::from(text)));
                        if crate::ui::window::is_visible() {
                            cx.notify();
                        }
                    });
                });
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
        let Some(pid) = spawn_daemon(stem, path, *secs) else {
            continue;
        };
        daemons.insert(
            stem.clone(),
            DaemonInfo {
                pid,
                path: path.clone(),
                secs: *secs,
            },
        );
    }
}

/// Spawn one daemon: a long-lived bash wrapper that sources the script
/// every `secs` seconds and emits its output delimited by a boundary line.
/// Returns the bash pid (registered for quit-time kill).
fn spawn_daemon(stem: &str, path: &Path, secs: u64) -> Option<u32> {
    let Some(tx) = OUTPUT_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    else {
        eprintln!("aerofi: scheduler: no dispatcher yet, skipping daemon for {stem}");
        return None;
    };

    let path_str = path.to_string_lossy().into_owned();
    let bash_script = format!(
        "while true; do\n  . \"{script}\" 2>/dev/null || true\n  printf '%s\\n' '{boundary}'\n  read -t {secs} _ 2>/dev/null || true\ndone",
        script = path_str,
        boundary = BOUNDARY,
        secs = secs,
    );

    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg(&bash_script)
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

    let path2 = path.to_path_buf();
    let stem2 = stem.to_string();
    // macOS default thread stack = 8 MB. Our loop only does
    // BufReader::lines() + channel send — 256 KB is plenty.
    let _ = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .name(format!("aerofi-inline:{}", path_str))
        .spawn(move || {
            let reader = BufReader::new(stdout);
            let mut buf = Vec::<String>::new();

            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if line.trim() == BOUNDARY {
                    let text = buf.join("\n");
                    let text = text.trim().to_string();
                    buf.clear();

                    if !text.is_empty() {
                        // UnboundedSender::unbounded_send never blocks.
                        if tx.unbounded_send((path2.clone(), text)).is_err() {
                            // Dispatcher gone — UI shut down.
                            break;
                        }
                    }
                } else if buf.len() < 500 {
                    buf.push(line);
                }
            }

            // Exit cleanup: kill (no-op if already dead, e.g. a reload
            // restarted us), reap the child, and drop the registry entries
            // if they still point at us.
            let _ = child.kill();
            let _ = child.wait();
            crate::core::executor::unregister_script(pid);
            daemon_remove(&stem2, pid);
        });

    Some(pid)
}
