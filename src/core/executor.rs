//! Execution of [`Target`]s.

use std::process::Command;

use crate::core::item::Target;

/// Run the given target.
///
/// Applications open via `open <path>`; scripts run as `sh <path>` with
/// stdio inherited, so their output is visible in the terminal aerofi was
/// launched from. Built-in actions are handled by the UI, not here.
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
