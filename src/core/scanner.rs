//! Directory indexing: scans the scripts folder and the standard macOS
//! application directories for launchable [`Target`]s.

use std::path::{Path, PathBuf};

use crate::core::item::Target;

/// Where user scripts live (relative to the working directory for the v0.1
/// PoC; v0.2 will take this from `config.toml`).
const SCRIPTS_DIR: &str = "examples/scripts";

/// Standard macOS application directories, in preference order.
const APP_DIRS: [&str; 2] = ["/Applications", "/System/Applications"];

/// Scan `dir` for shell scripts and parse their metadata into `Target::Script`s.
///
/// Returns an empty `Vec` (after logging) if the folder doesn't exist.
///
/// TODO: watch `dir` for changes (a notify-based watcher) so new scripts
/// appear without a restart.
pub fn scan_scripts(dir: &Path) -> Vec<Target> {
    let mut items = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("aerofi: scripts folder not found: {}", dir.display());
        return items;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_shell_script = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("sh" | "bash" | "zsh")
        );
        if !is_shell_script {
            continue;
        }
        if let Some(item) = Target::script_from_file(&path) {
            items.push(item);
        }
    }

    sort_by_name(&mut items);
    items
}

/// Scan the standard application directories (`/Applications`,
/// `/System/Applications`, `~/Applications`) for `.app` bundles and turn
/// each into a `Target::App`.
pub fn scan_applications() -> Vec<Target> {
    let mut dirs: Vec<PathBuf> = APP_DIRS.iter().map(PathBuf::from).collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("Applications"));
    }

    let mut items = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            items.push(Target::App {
                name,
                path,
                icon: Some("🚀".to_string()),
            });
        }
    }

    sort_by_name(&mut items);
    items
}

/// All launchable targets: applications + scripts, name-sorted.
pub fn scan_all() -> Vec<Target> {
    let mut targets = scan_applications();
    targets.extend(scan_scripts(Path::new(SCRIPTS_DIR)));
    sort_by_name(&mut targets);
    targets
}

fn sort_by_name(items: &mut [Target]) {
    items.sort_by(|a, b| a.name().cmp(b.name()));
}
