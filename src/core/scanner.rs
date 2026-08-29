//! Directory indexing: scans the configured scripts folders and the
//! standard macOS application directories for launchable [`Target`]s.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::core::config::AppConfig;
use crate::core::item::Target;

/// Standard macOS application directories, in preference order.
const APP_DIRS: [&str; 2] = ["/Applications", "/System/Applications"];

/// Scan all configured script folders for shell scripts and parse their
/// metadata into `Target::Script`s.
///
/// Returns an empty `Vec` when the scripts source is disabled in the
/// config. Missing folders are skipped (after logging); scripts from all
/// folders are collected into a single name-sorted list.
///
/// TODO: watch the folders for changes (a notify-based watcher) so new
/// scripts appear without a restart.
pub fn scan_scripts(config: &AppConfig) -> Vec<Target> {
    if !config.sources.scripts {
        return Vec::new();
    }

    let mut items = Vec::new();
    for dir in config.expanded_script_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            eprintln!("aerofi: scripts folder not found: {}", dir.display());
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = path.symlink_metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            if !is_shell_script(&path, &metadata) {
                continue;
            }
            if let Some(item) = Target::script_from_file(&path) {
                items.push(item);
            }
        }
    }

    sort_by_name(&mut items);
    items
}

/// A launchable script: has a shell extension, or the executable bit is
/// set (scripts run via `sh <script>`, so no extension is strictly
/// required).
fn is_shell_script(path: &Path, metadata: &std::fs::Metadata) -> bool {
    let has_shell_extension = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("sh" | "bash" | "zsh")
    );
    if has_shell_extension {
        return true;
    }
    metadata.permissions().mode() & 0o111 != 0
}

/// Scan the standard application directories (`/Applications`,
/// `/System/Applications`, `~/Applications`) for `.app` bundles and turn
/// each into a `Target::App`, skipping bundles whose name matches any
/// entry in `config.apps.ignored`.
///
/// Returns an empty `Vec` when the apps source is disabled in the config.
pub fn scan_applications(config: &AppConfig) -> Vec<Target> {
    if !config.sources.apps {
        return Vec::new();
    }

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
            if config
                .apps
                .ignored
                .iter()
                .any(|pattern| name_matches_pattern(pattern, &name))
            {
                continue;
            }
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

/// All launchable targets: applications + scripts + the built-in reload
/// action, name-sorted.
pub fn scan_all(config: &AppConfig) -> Vec<Target> {
    let mut targets = scan_applications(config);
    targets.extend(scan_scripts(config));
    targets.push(Target::reload_config());
    sort_by_name(&mut targets);
    targets
}

/// True when `name` matches `pattern`, case-insensitively. A pattern
/// without wildcards must match the whole name; `*` matches any run of
/// characters and `?` matches any single character.
fn name_matches_pattern(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let name: Vec<char> = name.to_ascii_lowercase().chars().collect();

    let (mut pi, mut ni) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;

    while ni < name.len() {
        if pi < pattern.len() && (pattern[pi] == name[ni] || pattern[pi] == '?') {
            pi += 1;
            ni += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(star_pos) = star {
            pi = star_pos + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

fn sort_by_name(items: &mut [Target]) {
    items.sort_by(|a, b| a.name().cmp(b.name()));
}
