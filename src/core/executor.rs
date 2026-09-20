//! Execution of [`Target`]s.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::core::item::{ScriptMode, Target};

/// Build the `Command` that runs a script: the interpreter from its
/// Put a spawned child in its own process group (pgid == child pid) so a
/// group kill can take down the child and everything it spawned.
#[cfg(unix)]
pub fn with_own_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(move || {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                // Never run in the inherited group: a group kill would
                // then hit aerofi's own processes. Abort the child.
                libc::_exit(127);
            }
        });
    }
}

#[cfg(not(unix))]
pub fn with_own_process_group(_cmd: &mut Command) {}

/// shebang (`#!/usr/bin/env bash`, `#!/usr/bin/env node`, ...). Without a
/// shebang, an executable file runs directly; a non-executable one falls
/// back to `sh` (the scanner accepts shell-extension files regardless of
/// the executable bit).
pub fn script_command(path: &Path) -> Command {
    let mut cmd = base_script_command(path);
    augment_script_path(&mut cmd);
    // Run each script in its own process group (pgid == child pid) so a
    // group kill can take down the script and everything it spawned.
    with_own_process_group(&mut cmd);
    cmd
}

fn base_script_command(path: &Path) -> Command {
    let content = std::fs::read_to_string(path).ok();
    if let Some(content) = content
        && let Some(first) = content.lines().next()
        && let Some(shebang) = first.strip_prefix("#!")
    {
        let parts: Vec<&str> = shebang.split_whitespace().collect();
        if !parts.is_empty() {
            let mut cmd = Command::new(parts[0]);
            cmd.args(&parts[1..]);
            cmd.arg(path);
            return cmd;
        }
    }
    use std::os::unix::fs::PermissionsExt;
    let executable = path
        .metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false);
    if executable {
        Command::new(path)
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg(path);
        cmd
    }
}

/// Prepend the Homebrew bin dirs to `PATH` for a spawned script.
///
/// When aerofi runs as a launchd/brew service its `PATH` is minimal
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), so a script's `#!/usr/bin/env python3`
/// shebang resolves to the old system Python rather than the user's Homebrew
/// one (and similarly for node, ripgrep, etc.). Prepending the Homebrew
/// prefixes — when present and not already on the path — restores the user's
/// toolchain for spawned scripts.
fn augment_script_path(cmd: &mut Command) {
    let current = std::env::var("PATH").unwrap_or_default();
    if let Some(path) = augmented_path(&current) {
        cmd.env("PATH", path);
    }
}

/// Return a `PATH` with the Homebrew bin dirs prepended (when present and not
/// already listed), or `None` when no change is needed.
fn augmented_path(current: &str) -> Option<String> {
    let existing: Vec<&str> = current.split(':').filter(|p| !p.is_empty()).collect();
    let extra: Vec<&str> = ["/opt/homebrew/bin", "/usr/local/bin"]
        .iter()
        .copied()
        .filter(|dir| Path::new(*dir).is_dir() && !existing.contains(dir))
        .collect();
    if extra.is_empty() {
        return None;
    }
    let mut combined = extra;
    combined.extend(existing);
    Some(combined.join(":"))
}

/// Process groups of every script currently running (pgid == child pid).
/// Background scripts are meant to outlive the launcher *window*, but they
/// must not outlive the process: [`kill_all_scripts`] takes them all down
/// on quit.
static RUNNING_SCRIPTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<u32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

fn lock_scripts() -> std::sync::MutexGuard<'static, std::collections::HashSet<u32>> {
    RUNNING_SCRIPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Track a spawned script's process group for quit-time cleanup.
pub fn register_script(pid: u32) {
    lock_scripts().insert(pid);
}

/// Forget a script once it has been reaped.
pub fn unregister_script(pid: u32) {
    lock_scripts().remove(&pid);
}

/// SIGKILL a single script's process group.
fn kill_script_group(pid: u32) {
    unsafe { libc::kill(-(pid as libc::c_int), libc::SIGKILL) };
}

/// Kill the process group of every still-running script (call on quit).
pub fn kill_all_scripts() {
    let mut guard = lock_scripts();
    let pids = std::mem::take(&mut *guard);
    for pid in pids {
        kill_script_group(pid);
    }
}

/// Serializes every test that spawns a real script process: `kill_all_scripts`
/// signals all registered groups, so no two such tests may share the registry.
#[cfg(test)]
pub(crate) static SCRIPT_PROCESS_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

/// True if `pid` is currently in the running-scripts registry.
#[cfg(test)]
pub fn script_is_registered(pid: u32) -> bool {
    lock_scripts().contains(&pid)
}

/// Output of a script run by [`run_bounded`].
pub struct ScriptOutput {
    /// Standard output. When the output exceeded the cap, the head is kept
    /// and a truncation note is appended.
    pub stdout: String,
    /// Standard error. When it exceeded the cap, the tail is kept and a
    /// truncation note is prepended (the interesting part of an error is at
    /// the end).
    pub stderr: String,
    /// Last non-empty stdout line, used by `compact`/`silent`/`inline`
    /// modes that only need a one-line summary.
    pub last_line: Option<String>,
    /// Whether the child exited with status 0.
    pub success: bool,
}

/// Max stdout bytes kept for display modes (`fullOutput`/`compact`/`silent`/
/// `inline`). Beyond this, the tail is noise in the launcher window — and
/// unbounded capture spikes RSS, which the allocator never returns.
pub const MAX_DISPLAY_OUTPUT: usize = 1024 * 1024; // 1 MiB
/// Max stdout bytes kept when piping to the clipboard. Clipboard payloads
/// are legitimately bigger than display ones, but a few MiB is already far
/// past any sane use.
pub const MAX_CLIPBOARD_OUTPUT: usize = 8 * 1024 * 1024; // 8 MiB
/// Max stderr bytes kept (tail). Any realistic stack trace fits.
const MAX_STDERR: usize = 256 * 1024; // 256 KiB
/// Tail kept to compute [`ScriptOutput::last_line`].
const LAST_LINE_TAIL: usize = 4096;

/// Open an application bundle via macOS `open`. When `new_instance` is true,
/// `-n` is passed so a fresh instance is launched even if one is already
/// running; otherwise `open` activates the existing instance.
pub fn open_app(path: &Path, name: &str, new_instance: bool) {
    let mut cmd = Command::new("open");
    if new_instance {
        cmd.arg("-n");
    }
    cmd.arg(path);
    if let Err(e) = cmd.status() {
        eprintln!("aerofi: failed to run {name}: {e}");
    }
}

/// Run the script at `path` and capture its output with streaming readers
/// whose memory use is bounded by `stdout_cap`, no matter how much the
/// script writes.
///
/// `stdout` is truncated from the tail (head kept), `stderr` from the head
/// (tail kept), and `last_line` is the last non-empty stdout line. The
/// child's pipes are always fully drained, so chatty scripts never block on
/// a full pipe buffer.
pub fn run_bounded(
    path: &Path,
    args: &[String],
    stdout_cap: usize,
) -> std::io::Result<ScriptOutput> {
    let mut child = script_command(path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let script_pid = child.id();
    register_script(script_pid);
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    // Drain stderr concurrently so a script writing a lot to both streams
    // never blocks on a full pipe buffer.
    let stderr_thread = std::thread::Builder::new()
        .name("aerofi-stderr".into())
        .spawn(move || read_tail_bounded(stderr, MAX_STDERR))
        .expect("failed to spawn stderr drain thread");

    let read_result = read_capped(stdout, stdout_cap);
    let stderr = stderr_thread.join().expect("stderr drain thread panicked");
    use std::os::unix::process::ExitStatusExt;
    let status = child
        .wait()
        .unwrap_or_else(|_| std::process::ExitStatus::from_raw(1));
    unregister_script(script_pid);
    let (head, tail, total, truncated) = read_result?;

    let mut stdout = String::from_utf8_lossy(&head).into_owned();
    if truncated {
        stdout.push_str(&format!(
            "\n\n… [output truncated: showing first {} of {}]",
            human_size(head.len() as u64),
            human_size(total)
        ));
    }
    let last_line = last_nonempty_line(&tail);

    Ok(ScriptOutput {
        stdout,
        stderr,
        last_line,
        success: status.success(),
    })
}

/// Read a stream to EOF, keeping the first `cap` bytes (head) and the last
/// `LAST_LINE_TAIL` bytes (for line-based summaries). Returns
/// `(head, tail, total bytes read, whether truncated)`.
fn read_capped<R: Read>(
    mut reader: R,
    cap: usize,
) -> std::io::Result<(Vec<u8>, Vec<u8>, u64, bool)> {
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut buf = [0u8; 8192];
    let mut total: u64 = 0;
    let mut truncated = false;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        let chunk = &buf[..n];
        if head.len() < cap {
            let take = cap.saturating_sub(head.len()).min(n);
            head.extend_from_slice(&chunk[..take]);
            if take < n {
                truncated = true;
            }
        } else {
            truncated = true;
        }
        tail.extend_from_slice(chunk);
        if tail.len() > LAST_LINE_TAIL {
            tail.drain(..tail.len() - LAST_LINE_TAIL);
        }
    }
    Ok((head, tail, total, truncated))
}

/// Read a stream to EOF, keeping only the last `cap` bytes. Returns the
/// kept text and whether anything was dropped.
fn read_tail_bounded<R: Read>(mut reader: R, cap: usize) -> String {
    let mut tail = Vec::new();
    let mut buf = [0u8; 8192];
    let mut truncated = false;
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > cap {
            tail.drain(..tail.len() - cap);
            truncated = true;
        }
    }
    let text = String::from_utf8_lossy(&tail).into_owned();
    if truncated {
        format!("… [stderr truncated]\n{text}")
    } else {
        text
    }
}

/// Last non-empty line of a byte tail. The tail is the end of the stream,
/// so the last line is complete unless it alone exceeds the tail, in which
/// case the best available fragment is returned.
fn last_nonempty_line(tail: &[u8]) -> Option<String> {
    String::from_utf8_lossy(tail)
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(|s| s.to_string())
}

/// Format a byte count for human consumption (e.g. `1.2 MiB`).
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GiB", b / GB)
    } else if b >= MB {
        format!("{:.1} MiB", b / MB)
    } else if b >= KB {
        format!("{:.0} KiB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Run the given target asynchronously or detached.
///
/// Applications open via `open <path>`; scripts run through their shebang
/// interpreter. `pipe` scripts capture stdout and copy it to the clipboard
/// (`pbcopy`), matching the launcher path. Failures are reported to
/// stderr, never panics.
pub fn execute(target: &Target) {
    match target {
        Target::App { path, .. } => open_app(path, target.name(), false),
        Target::Script {
            mode: ScriptMode::Pipe,
            path,
            ..
        } => {
            // Pipe to clipboard without blocking the main thread (the
            // global hotkey handler runs on it).
            let path = path.clone();
            let name = target.name().to_string();
            let thread_name = name.clone();
            let spawn_result = std::thread::Builder::new()
                .name(format!("aerofi-pipe:{name}"))
                .stack_size(256 * 1024)
                .spawn(move || {
                    let mut child = match script_command(&path)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("aerofi: failed to run {thread_name}: {e}");
                            return;
                        }
                    };
                    let script_pid = child.id();
                    register_script(script_pid);
                    let Ok(mut pbcopy) = Command::new("pbcopy").stdin(Stdio::piped()).spawn()
                    else {
                        eprintln!("aerofi: failed to spawn pbcopy for {thread_name}");
                        // Drain the child's pipe so it never blocks on a
                        // full buffer.
                        if let Some(mut stdout) = child.stdout.take() {
                            let mut sink = std::io::sink();
                            let _ = std::io::copy(&mut stdout, &mut sink);
                        }
                        let _ = child.wait();
                        unregister_script(script_pid);
                        return;
                    };
                    // Stream stdout straight into pbcopy so an arbitrarily
                    // large script output never accumulates in our memory.
                    if let (Some(mut stdout), Some(mut stdin)) =
                        (child.stdout.take(), pbcopy.stdin.take())
                    {
                        let _ = std::io::copy(&mut stdout, &mut stdin);
                    }
                    let _ = child.wait();
                    unregister_script(script_pid);
                    let _ = pbcopy.wait();
                });
            if let Err(e) = spawn_result {
                eprintln!("aerofi: failed to spawn pipe thread for {name}: {e}");
            }
        }
        Target::Script { path, .. } => match script_command(path).spawn() {
            Ok(mut child) => {
                let name = target.name().to_string();
                let script_pid = child.id();
                register_script(script_pid);
                let _ = std::thread::Builder::new()
                    .name(format!("aerofi-reap:{name}"))
                    .stack_size(128 * 1024)
                    .spawn(move || {
                        let _ = child.wait();
                        unregister_script(script_pid);
                    });
            }
            Err(e) => eprintln!("aerofi: failed to run {}: {e}", target.name()),
        },
        // Built-in actions are handled by the UI, never executed here.
        Target::Builtin { .. } => {}
        Target::PluginItem { .. } => {
            // Plugins are executed by the PluginManager via LauncherAction::ActivatePlugin,
            // so we do nothing here.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_script(label: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("aerofi_exec_{label}_{}", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        use std::io::Write as _;
        writeln!(file, "{content}").unwrap();
        path
    }

    #[test]
    fn shebang_selects_interpreter_and_args() {
        let path = temp_script("shebang", "#!/usr/bin/env bash");
        let cmd = script_command(&path);
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(program, "/usr/bin/env");
        assert_eq!(
            args,
            vec!["bash".to_string(), path.to_string_lossy().into_owned()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn no_shebang_non_executable_falls_back_to_sh() {
        // Freshly created temp files are not executable, so a shebang-less
        // shell script must run under `sh` (previously every script did).
        let path = temp_script("plain", "echo hi");
        let cmd = script_command(&path);
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(program, "sh");
        assert_eq!(args, vec![path.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn no_shebang_executable_runs_directly() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_script("exec", "echo hi");
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        let cmd = script_command(&path);
        assert_eq!(
            cmd.get_program().to_string_lossy().into_owned(),
            path.to_string_lossy().into_owned()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn augmented_path_prepends_homebrew_and_keeps_original() {
        // Only meaningful on a machine that has a Homebrew prefix.
        let homebrew = std::path::Path::new("/opt/homebrew/bin");
        if !homebrew.is_dir() && !std::path::Path::new("/usr/local/bin").is_dir() {
            return;
        }
        let current = "/usr/bin:/bin:/usr/sbin:/sbin";
        let result = augmented_path(current).expect("expected a change");
        let expected_prefix = if homebrew.is_dir() {
            "/opt/homebrew/bin:"
        } else {
            "/usr/local/bin:"
        };
        assert!(result.starts_with(expected_prefix), "got {result}");
        assert!(result.ends_with(current), "got {result}");
        let parts: Vec<&str> = result.split(':').collect();
        let unique: std::collections::HashSet<_> = parts.iter().copied().collect();
        assert_eq!(parts.len(), unique.len(), "duplicate entries in {result}");
    }

    #[test]
    fn augmented_path_none_when_prefixes_already_present() {
        // Both prefixes already listed → nothing to add → None.
        let current = "/opt/homebrew/bin:/usr/local/bin:/usr/bin";
        assert!(augmented_path(current).is_none());
    }

    #[test]
    fn run_bounded_small_output_is_verbatim() {
        let _lock = SCRIPT_PROCESS_LOCK.lock().unwrap();
        let path = temp_script(
            "bounded_small",
            "#!/usr/bin/env bash\necho line1\necho line2",
        );
        let out = run_bounded(&path, &[], MAX_DISPLAY_OUTPUT).unwrap();
        assert!(out.success);
        assert_eq!(out.stdout, "line1\nline2\n");
        assert_eq!(out.last_line.as_deref(), Some("line2"));
        assert!(out.stderr.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_bounded_truncates_large_stdout_and_keeps_head() {
        let _lock = SCRIPT_PROCESS_LOCK.lock().unwrap();
        // `seq 1 200000` is ~1.23 MiB — just over the 1 MiB display cap.
        let path = temp_script("bounded_large", "#!/usr/bin/env bash\nseq 1 200000");
        let out = run_bounded(&path, &[], MAX_DISPLAY_OUTPUT).unwrap();
        assert!(out.success);
        assert_eq!(out.last_line.as_deref(), Some("200000"));
        let expected_head: String = (1..=200_000).map(|i| format!("{i}\n")).collect();
        assert!(
            out.stdout.starts_with(&expected_head[..MAX_DISPLAY_OUTPUT]),
            "the head of the output must be kept verbatim"
        );
        assert!(out.stdout.contains("[output truncated"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_bounded_keeps_stderr_tail() {
        let _lock = SCRIPT_PROCESS_LOCK.lock().unwrap();
        // ~600 KiB of stderr, well over the 256 KiB cap: the head is
        // dropped, the tail (with the real error) survives.
        let path = temp_script(
            "bounded_err",
            "#!/usr/bin/env bash\nseq 1 100000 >&2\necho final-error >&2",
        );
        let out = run_bounded(&path, &[], MAX_DISPLAY_OUTPUT).unwrap();
        assert!(out.success);
        assert!(
            out.stderr.starts_with("… [stderr truncated]"),
            "stderr should carry the truncation note"
        );
        assert!(out.stderr.contains("100000"));
        assert!(out.stderr.ends_with("final-error\n"));
        assert!(out.last_line.is_none(), "no stdout was produced");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_bounded_reports_failure_status() {
        let _lock = SCRIPT_PROCESS_LOCK.lock().unwrap();
        let path = temp_script("bounded_fail", "#!/usr/bin/env bash\necho oops\nexit 3");
        let out = run_bounded(&path, &[], MAX_DISPLAY_OUTPUT).unwrap();
        assert!(!out.success);
        assert_eq!(out.stdout, "oops\n");
        assert_eq!(out.last_line.as_deref(), Some("oops"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_bounded_registers_and_unregisters_script() {
        let _lock = SCRIPT_PROCESS_LOCK.lock().unwrap();
        // A script that blocks: while it runs, its pid must be in the
        // registry; after run_bounded returns, it must be gone.
        let path = temp_script("registry", "#!/usr/bin/env bash\nsleep 2\necho done");
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let out = run_bounded(&path, &[], MAX_DISPLAY_OUTPUT).unwrap();
                assert!(out.success);
            });
            // Wait for the script to actually be running (generous window:
            // the script sleeps 2 s, so the registry stays populated long
            // enough even under heavy CI load).
            for _ in 0..500 {
                if !lock_scripts().is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                !lock_scripts().is_empty(),
                "script not registered while running"
            );
            handle.join().unwrap();
        });
        assert!(
            lock_scripts().is_empty(),
            "script not unregistered after run"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn kill_all_scripts_drains_registry_without_hitting_other_groups() {
        let _lock = SCRIPT_PROCESS_LOCK.lock().unwrap();
        // A plain child shares THIS process's group, so its pid is not a
        // group id: kill(-pid) is a guaranteed ESRCH no-op. Registering it
        // simulates a stale registry entry and proves kill_all_scripts
        // drains the registry without signalling an unrelated group.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        register_script(pid);
        kill_all_scripts();
        assert!(lock_scripts().is_empty(), "registry not drained");
        let _ = child.wait();
    }
}
