//! Execution of [`Target`]s.

use std::process::Command;

use crate::core::item::Target;

/// Run the given target.
///
/// Applications open via `open <path>`; scripts run as `sh <path>` with
/// stdio inherited, so their output is visible in the terminal aerofi was
/// launched from. Failures are reported to stderr, never panics.
pub fn execute(target: &Target) {
    let spawn = match target {
        Target::App { path, .. } => Command::new("open").arg(path).spawn(),
        Target::Script { path, .. } => Command::new("sh").arg(path).spawn(),
    };
    if let Err(e) = spawn {
        eprintln!("aerofi: failed to run {}: {e}", target.name());
    }
}
