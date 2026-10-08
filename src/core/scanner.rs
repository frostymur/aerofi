use gpui::SharedString;
use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::config::AppConfig;
use crate::core::item::Target;

/// Standard macOS application directories, in preference order.
const APP_DIRS: [&str; 4] = [
    "/Applications",
    "/Applications/Utilities",
    "/System/Applications",
    "/System/Applications/Utilities",
];

/// Scan all configured script folders for shell scripts and parse their
/// metadata into `Target::Script`s.
///
/// Returns an empty `Vec` when the scripts source is disabled in the
/// config. Missing folders are skipped (after logging); scripts from all
/// folders are collected into a single name-sorted list.
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
    if metadata.permissions().mode() & 0o111 != 0 {
        return true;
    }
    let extension = path.extension().and_then(|e| e.to_str());
    matches!(
        extension,
        Some("sh" | "bash" | "zsh" | "py" | "rb" | "js" | "applescript" | "scpt")
    )
}

/// Scan the standard application directories (`/Applications`,
/// `/System/Applications`, and their `Utilities/` subdirectories, plus
/// `~/Applications`), custom `config.apps.extra_dirs`, and individual bundles
/// in `config.apps.extra_apps` for `.app` bundles, skipping bundles whose name
/// matches any entry in `config.apps.ignored`. Duplicate bundles across
/// scanned locations are automatically deduplicated.
///
/// Returns an empty `Vec` when the apps source is disabled in the config.
pub fn scan_applications(config: &AppConfig) -> Vec<Target> {
    if !config.sources.apps {
        return Vec::new();
    }

    let mut dirs: Vec<PathBuf> = APP_DIRS.iter().map(PathBuf::from).collect();
    if let Some(home) = dirs::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
        let user_apps = home.join("Applications");
        let user_utils = user_apps.join("Utilities");
        dirs.push(user_apps);
        dirs.push(user_utils);
    }
    dirs.extend(config.expanded_app_dirs());

    let mut seen = HashSet::new();
    let mut items = Vec::new();

    let ignore_dirs = config.expanded_ignore_dirs();
    let mut add_app = |path: PathBuf, items: &mut Vec<Target>| {
        // Skip paths that are inside any ignored directory
        if ignore_dirs.iter().any(|d| path.starts_with(d)) {
            return;
        }

        if !path.is_dir() || path.extension().and_then(|e| e.to_str()) != Some("app") {
            return;
        }
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            return;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if config
            .apps
            .ignore_names
            .iter()
            .any(|pattern| name_matches_pattern(pattern, &name))
        {
            return;
        }
        items.push(Target::App {
            name: SharedString::from(name),
            path: Arc::from(path),
            icon_path: None,
        });
    };

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            add_app(entry.path(), &mut items);
        }
    }

    for app_path in config.expanded_extra_apps() {
        add_app(app_path, &mut items);
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

/// Replace the `Target::Script` entries of the base list (the first
/// `base_count` items of `all`) with freshly scanned `new_scripts` and
/// re-sort the base. Plugin rows beyond `base_count` are kept in place.
/// Returns the new base count. Used by the script-folder watcher's rescan
/// (`Launcher::rescan_scripts`).
pub(crate) fn replace_scripts(
    all: &mut Vec<Target>,
    base_count: usize,
    new_scripts: Vec<Target>,
) -> usize {
    let plugin_tail: Vec<Target> = all.split_off(base_count);
    all.retain(|t| !matches!(t, Target::Script { .. }));
    all.extend(new_scripts);
    sort_by_name(all);
    let base = all.len();
    all.extend(plugin_tail);
    base
}

/// True when `name` matches `pattern`, case-insensitively. A pattern
/// without wildcards must match the whole name; `*` matches any run of
/// characters and `?` matches any single character.
pub(crate) fn name_matches_pattern(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let name: Vec<char> = name.to_ascii_lowercase().chars().collect();
    glob_match_chars(&pattern, &name)
}

/// Core of [`name_matches_pattern`] over pre-lowercased char slices.
/// Allocation-free: the search path calls it per target per keystroke and
/// reuses its own scratch buffers.
pub(crate) fn glob_match_chars(pattern: &[char], name: &[char]) -> bool {
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

pub(crate) fn sort_by_name(items: &mut [Target]) {
    items.sort_by(|a, b| a.name().cmp(b.name()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_matching_works() {
        assert!(name_matches_pattern("Uninstall*", "Uninstall App"));
        assert!(name_matches_pattern("*Helper", "RendererHelper"));
        assert!(name_matches_pattern("Console", "Console"));
        assert!(name_matches_pattern("console", "Console"));
        assert!(!name_matches_pattern("Console", "Activity Monitor"));
        assert!(name_matches_pattern("?onsole", "Console"));
    }

    #[test]
    fn scan_applications_includes_utilities_and_custom_apps() {
        let mut config = AppConfig::default();
        config.sources.apps = true;

        let apps = scan_applications(&config);
        // On standard macOS, /System/Applications/Utilities contains Activity Monitor and Console
        let has_activity_monitor = apps.iter().any(|app| app.name() == "Activity Monitor");
        let has_console = apps.iter().any(|app| app.name() == "Console");
        assert!(
            has_activity_monitor || has_console,
            "scan_applications should automatically discover utilities like Activity Monitor or Console"
        );
    }

    struct TempTestDir(PathBuf);
    impl TempTestDir {
        fn new(name: &str) -> Self {
            let p =
                std::env::temp_dir().join(format!("aerofi-test-{}-{}", name, std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scan_applications_supports_extra_dirs_and_extra_apps() {
        let temp_dir = TempTestDir::new("scanner");
        let custom_app_dir = temp_dir.path().join("Custom.app");
        std::fs::create_dir_all(&custom_app_dir).unwrap();

        let explicit_app_dir = temp_dir.path().join("Explicit.app");
        std::fs::create_dir_all(&explicit_app_dir).unwrap();

        let mut config = AppConfig::default();
        config.apps.extra_dirs = vec![temp_dir.path().to_path_buf()];
        config.apps.extra_apps = vec![explicit_app_dir.clone()];

        let apps = scan_applications(&config);
        assert!(apps.iter().any(|a| a.name() == "Custom"));
        assert!(apps.iter().any(|a| a.name() == "Explicit"));

        // Test deduplication: explicit app already found shouldn't appear twice
        let explicit_matches = apps.iter().filter(|a| a.name() == "Explicit").count();
        assert_eq!(explicit_matches, 1);
    }

    #[test]
    fn replace_scripts_swaps_scripts_and_preserves_plugin_tail() {
        let temp = TempTestDir::new("splice");
        let alpha = temp.path().join("alpha.sh");
        let beta = temp.path().join("beta.sh");
        std::fs::write(&alpha, "#!/bin/sh\necho alpha\n").unwrap();
        std::fs::write(&beta, "#!/bin/sh\necho beta\n").unwrap();

        let app = Target::App {
            name: SharedString::from("Zed"),
            path: Arc::from(PathBuf::from("/Applications/Zed.app")),
            icon_path: None,
        };
        let mut all = vec![
            app,
            Target::script_from_file(&alpha).unwrap(),
            Target::reload_config(),
        ];
        all.push(Target::PluginItem {
            name: SharedString::from("Plugin row"),
            subtitle: None,
            icon: None,
            plugin_id: SharedString::from("1"),
            plugin_name: SharedString::from("demo"),
        });
        let base_count = 3;

        // New scan after alpha.sh was deleted: only beta remains.
        std::fs::remove_file(&alpha).unwrap();
        let new_scripts = vec![Target::script_from_file(&beta).unwrap()];
        let new_base = replace_scripts(&mut all, base_count, new_scripts);

        assert_eq!(new_base, 3);
        assert_eq!(all.len(), 4);
        // The base is re-sorted (byte order: uppercase before lowercase).
        assert_eq!(all[0].name(), "Reload Configuration");
        assert_eq!(all[1].name(), "Zed");
        assert_eq!(all[2].name(), "beta");
        // The plugin row beyond base_count survives untouched.
        assert!(matches!(all[3], Target::PluginItem { .. }));
    }
}
