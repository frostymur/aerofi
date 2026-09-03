//! Execution of [`Target`]s.

use std::path::Path;
use std::process::Command;

use crate::core::item::Target;

/// Build the `Command` that runs a script: the interpreter from its
/// shebang (`#!/usr/bin/env bash`, `#!/usr/bin/env node`, ...). Without a
/// shebang, an executable file runs directly; a non-executable one falls
/// back to `sh` (the scanner accepts shell-extension files regardless of
/// the executable bit).
pub fn script_command(path: &Path) -> Command {
    let content = std::fs::read_to_string(path).ok();
    if let Some(content) = content {
        if let Some(first) = content.lines().next() {
            if let Some(shebang) = first.strip_prefix("#!") {
                let parts: Vec<&str> = shebang.split_whitespace().collect();
                if !parts.is_empty() {
                    let mut cmd = Command::new(parts[0]);
                    cmd.args(&parts[1..]);
                    cmd.arg(path);
                    return cmd;
                }
            }
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

/// Run the given target asynchronously or detached.
///
/// Applications open via `open <path>`; scripts run as `sh <path>`.
/// Failures are reported to stderr, never panics.
pub fn execute(target: &Target) {
    let (program, path) = match target {
        Target::App { path, .. } => ("open", path),
        Target::Script { path, .. } => ("sh", path),
        // Built-in actions are handled by the UI, never executed here.
        Target::Builtin { .. } => return,
    };
    if let Err(e) = Command::new(program).arg(&**path).spawn() {
        eprintln!("aerofi: failed to run {}: {e}", target.name());
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
}
