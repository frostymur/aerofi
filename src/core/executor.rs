//! Execution of [`Target`]s.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::core::item::{ScriptMode, Target};

/// Build the `Command` that runs a script: the interpreter from its
/// shebang (`#!/usr/bin/env bash`, `#!/usr/bin/env node`, ...). Without a
/// shebang, an executable file runs directly; a non-executable one falls
/// back to `sh` (the scanner accepts shell-extension files regardless of
/// the executable bit).
pub fn script_command(path: &Path) -> Command {
    let mut cmd = base_script_command(path);
    augment_script_path(&mut cmd);
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
                    let output = script_command(&path).output();
                    let text = match output {
                        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
                        Err(e) => {
                            eprintln!("aerofi: failed to run {thread_name}: {e}");
                            return;
                        }
                    };
                    let Ok(mut pbcopy) = Command::new("pbcopy").stdin(Stdio::piped()).spawn()
                    else {
                        eprintln!("aerofi: failed to spawn pbcopy for {thread_name}");
                        return;
                    };
                    let _ = pbcopy
                        .stdin
                        .take()
                        .map(|mut stdin| stdin.write_all(text.as_bytes()));
                    let _ = pbcopy.wait();
                });
            if let Err(e) = spawn_result {
                eprintln!("aerofi: failed to spawn pipe thread for {name}: {e}");
            }
        }
        Target::Script { path, .. } => match script_command(path).spawn() {
            Ok(mut child) => {
                let name = target.name().to_string();
                let _ = std::thread::Builder::new()
                    .name(format!("aerofi-reap:{name}"))
                    .stack_size(128 * 1024)
                    .spawn(move || {
                        let _ = child.wait();
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
}
