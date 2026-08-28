//! Directory indexing: scans the scripts folder for shell scripts and
//! parses their metadata.

use std::path::Path;

use crate::common::script_item::ScriptItem;

/// Scan `dir` for shell scripts and parse their metadata.
///
/// Returns an empty `Vec` (after logging) if the folder doesn't exist.
///
/// TODO: watch `dir` for changes (a notify-based watcher) so new scripts
/// appear without a restart.
pub fn scan_scripts(dir: &Path) -> Vec<ScriptItem> {
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
        if let Some(item) = ScriptItem::from_file(&path) {
            items.push(item);
        }
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}
