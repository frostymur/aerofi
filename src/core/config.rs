//! Hand-edited TOML configuration. v1 has no settings GUI: the file is
//! created with defaults on first run and lives at
//! `~/.config/aerofi/config.toml`.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "config.toml";

/// Pretty default config (with comments) written on first run.
const DEFAULT_CONFIG: &str = r#"# aerofi configuration
# Hand-edited file: there is no settings GUI (see ARCHITECTURE.md).
# Restart aerofi after editing to apply changes.

# Theme name: resolved to ~/.config/aerofi/themes/{name}.toml.
# Use "default" for the built-in Tokyo Night palette.
theme = "default"

# Pin items to the top of the results, in the order listed. Each entry
# matches a target by display name (case-insensitive) or by full path, e.g.:
# pinned = ["Safari", "Power Menu"]
pinned = []

[general]
# Maximum number of results shown in the launcher list.
max_results = 20
# Launch new app instances (shift+enter) in the background (open -n -g):
# the window opens on the current workspace without activating the app, so
# macOS won't switch to another workspace where the app is already open.
background_new_instance = false
# How the filter query matches target names/aliases:
# "fuzzy" (default, fzf-style), "prefix" (must start with the query),
# or "glob" (* = any run of characters, ? = one character).
matching = "fuzzy"
# How results are ordered: "frecency" (default: fuzzy score + usage history)
# or "lexical" (alphabetical; pinned items still lead).
ranking = "frecency"

[sources]
# Which target sources the launcher indexes.
apps = true
scripts = true
system_settings = true

[scripts]
# Folders scanned for launchable scripts. A leading `~` is expanded
# to the user's home directory.
dirs = ["~/.config/aerofi/scripts"]

[apps]
# Application bundle names hidden from the launcher.
# `*` matches any run of characters, `?` matches any single character;
# entries without wildcards are exact names.
ignored = ["Uninstall*", "Installer"]
# Additional folders scanned for .app bundles (e.g. "~/Applications", "/opt/homebrew/Applications")
extra_dirs = []
# Explicit individual .app bundles to include
extra_apps = []

[aliases]
# Alternate names for targets. Typing an alias finds the target in the
# search; typing it exactly runs the target immediately (no Enter), e.g.:
# "rc" = "Reload Configuration"

[bindings]
# Global hotkey to toggle the launcher visibility.
# Examples: "opt+space", "cmd+space", "ctrl+shift+p"
toggle = "opt+space"

# Key combinations that run a target immediately while the launcher is
# open. Modifiers: cmd, ctrl, alt, shift (any order, before the key), e.g.:
# "cmd+r" = "Reload Configuration"
[bindings.launcher]

# System-wide shortcuts: run the named target directly, without opening the
# launcher. Use opt (or cmd) to avoid app conflicts, e.g.:
# "opt+d" = "Deploy"
# Registered at startup only (restart after editing); conflicting combos
# are skipped with a warning.
[bindings.global]

# Bind combinations to emit custom action events (retv: 10..28) in rofi mode.
# e.g. "kb-custom-1" = "alt+1"
[bindings.custom]
"#;

/// How the filter query is matched against target names and aliases.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// Fuzzy subsequence match (nucleo, fzf-style scoring) — the default.
    #[default]
    Fuzzy,
    /// The name (or an alias) must start with the query.
    Prefix,
    /// The name (or an alias) must match the glob: `*` = any run of
    /// characters, `?` = any single character. Without wildcards this is an
    /// exact match.
    Glob,
}

/// How matching targets are ordered in the list.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingMode {
    /// Fuzzy score + frecency (history) — the default.
    #[default]
    Frecency,
    /// Alphabetical by display name. Pinned items still lead.
    Lexical,
}

/// Launcher-wide behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// The maximum number of search results displayed in the UI.
    pub max_results: usize,
    /// Launch new app instances (Shift+Enter) in the background via
    /// `open -n -g`: the window appears on the current workspace without
    /// activating the app, so macOS won't switch to another workspace where
    /// the app is already open (useful with tiling window managers).
    pub background_new_instance: bool,
    /// Filter query matching mode: `fuzzy` (default), `prefix`, or `glob`.
    pub matching: MatchMode,
    /// Result ranking: `frecency` (default) or `lexical` (alphabetical).
    pub ranking: RankingMode,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            max_results: 20,
            background_new_instance: false,
            matching: MatchMode::Fuzzy,
            ranking: RankingMode::Frecency,
        }
    }
}

/// Which target sources the launcher indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    /// Application bundles from the standard macOS directories.
    pub apps: bool,
    /// Shell scripts from the configured script folders.
    pub scripts: bool,
    /// System settings entries (reserved for a later release).
    pub system_settings: bool,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            apps: true,
            scripts: true,
            system_settings: true,
        }
    }
}

/// Script source settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptsConfig {
    /// Folders scanned for scripts. A leading `~` is expanded via
    /// [`AppConfig::expanded_script_dirs`].
    pub dirs: Vec<PathBuf>,
}

/// Application source settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppsConfig {
    /// Names (or `*`/`?` patterns) of bundles hidden from the launcher.
    #[serde(alias = "ignored")]
    pub ignore_names: Vec<String>,
    /// Directories completely excluded from scanning (tilde `~` expanded).
    pub ignore_dirs: Vec<PathBuf>,
    /// Additional folders scanned for `.app` bundles (tilde `~` expanded).
    pub extra_dirs: Vec<PathBuf>,
    /// Explicit paths to individual `.app` bundles to include (tilde `~` expanded).
    pub extra_apps: Vec<PathBuf>,
}

impl Default for AppsConfig {
    fn default() -> Self {
        Self {
            ignore_names: vec!["Uninstall*".to_string(), "Installer".to_string()],
            ignore_dirs: Vec::new(),
            extra_dirs: Vec::new(),
            extra_apps: Vec::new(),
        }
    }
}

/// Unified keybindings configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BindingsConfig {
    /// Global hotkey used to toggle the launcher.
    pub toggle: String,
    /// Hotkeys active while the launcher is open (combo -> target name).
    pub launcher: HashMap<String, String>,
    /// System-wide global hotkeys (combo -> target name).
    pub global: HashMap<String, String>,
    /// Custom hotkeys for Rofi scripts (kb-custom-N -> combo).
    pub custom: HashMap<String, String>,
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self {
            toggle: "opt+space".to_string(),
            launcher: HashMap::new(),
            global: HashMap::new(),
            custom: HashMap::new(),
        }
    }
}

/// Root configuration, stored as `~/.config/aerofi/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Theme name resolved to `~/.config/aerofi/themes/{name}.toml`.
    pub theme: String,
    pub general: GeneralConfig,
    pub sources: SourcesConfig,
    pub scripts: ScriptsConfig,
    pub apps: AppsConfig,
    /// Alternate names for targets (alias -> target name).
    pub aliases: HashMap<String, String>,
    /// Unified keybindings configuration.
    pub bindings: BindingsConfig,
    /// Items pinned to the top of the results, in the order listed. Each
    /// entry matches a target by display name (case-insensitive) or by full
    /// path. Pinned items outrank frecency and fuzzy score while matching the
    /// current query.
    pub pinned: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
            general: GeneralConfig::default(),
            sources: SourcesConfig::default(),
            scripts: ScriptsConfig::default(),
            apps: AppsConfig::default(),
            aliases: HashMap::new(),
            bindings: BindingsConfig::default(),
            pinned: Vec::new(),
        }
    }
}

impl AppConfig {
    /// `scripts.dirs` with a leading `~` replaced by the user's home
    /// directory (`dirs::home_dir()`). Paths without a leading `~` are
    /// returned unchanged.
    pub fn expanded_script_dirs(&self) -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return self.scripts.dirs.clone();
        };
        self.scripts
            .dirs
            .iter()
            .map(|dir| expand_tilde(dir, &home))
            .collect()
    }

    /// `apps.extra_dirs` with a leading `~` replaced by the user's home
    /// directory. Paths without a leading `~` are returned unchanged.
    pub fn expanded_app_dirs(&self) -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return self.apps.extra_dirs.clone();
        };
        self.apps
            .extra_dirs
            .iter()
            .map(|dir| expand_tilde(dir, &home))
            .collect()
    }

    /// `apps.extra_apps` with a leading `~` replaced by the user's home
    /// directory. Paths without a leading `~` are returned unchanged.
    pub fn expanded_extra_apps(&self) -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return self.apps.extra_apps.clone();
        };
        self.apps
            .extra_apps
            .iter()
            .map(|app| expand_tilde(app, &home))
            .collect()
    }

    /// `apps.ignore_dirs` with a leading `~` replaced by the user's home
    /// directory. Paths without a leading `~` are returned unchanged.
    pub fn expanded_ignore_dirs(&self) -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return self.apps.ignore_dirs.clone();
        };
        self.apps
            .ignore_dirs
            .iter()
            .map(|dir| expand_tilde(dir, &home))
            .collect()
    }

    /// Load the config from `~/.config/aerofi/config.toml`.
    ///
    /// If the file doesn't exist, its directory is created, a commented
    /// default config is written, and the primary scripts folder is
    /// created. If the file exists it is read and parsed; on a read or
    /// parse error a warning is printed and [`AppConfig::default`] is
    /// returned instead.
    pub fn load() -> AppConfig {
        let config_path = config_path();
        let config = if config_path.is_file() {
            match fs::read_to_string(&config_path) {
                Ok(contents) => match toml::from_str::<AppConfig>(&contents) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        eprintln!(
                            "aerofi: warning: failed to parse {}: {err}; using defaults",
                            config_path.display()
                        );
                        AppConfig::default()
                    }
                },
                Err(err) => {
                    eprintln!(
                        "aerofi: warning: failed to read {}: {err}; using defaults",
                        config_path.display()
                    );
                    AppConfig::default()
                }
            }
        } else {
            let defaults = toml::from_str::<AppConfig>(DEFAULT_CONFIG).unwrap_or_default();
            let parent = config_path.parent().map(PathBuf::from).unwrap_or_default();
            if let Err(err) =
                fs::create_dir_all(&parent).and_then(|()| fs::write(&config_path, DEFAULT_CONFIG))
            {
                eprintln!("aerofi: warning: failed to write default config: {err}");
            }
            defaults
        };

        if let Some(first) = config.expanded_script_dirs().first()
            && let Err(err) = fs::create_dir_all(first)
        {
            eprintln!(
                "aerofi: warning: failed to create scripts dir {}: {err}",
                first.display()
            );
        }

        if let Some(parent) = config_path.parent() {
            let themes_dir = parent.join("themes");
            if !themes_dir.exists() {
                let _ = fs::create_dir_all(&themes_dir);
            }
            let plugins_dir = parent.join("plugins");
            if !plugins_dir.exists() {
                let _ = fs::create_dir_all(&plugins_dir);
            }
        }

        config
    }
}

/// Path of the config file: `~/.config/aerofi/config.toml`.
fn config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("aerofi").join(CONFIG_FILE_NAME)
}

/// Replace a leading `~` component with `home`; any other path is returned
/// unchanged.
fn expand_tilde(dir: &Path, home: &Path) -> PathBuf {
    let mut components = dir.components();
    match components.next() {
        Some(Component::Normal(tilde)) if tilde == "~" => {
            home.join(components.collect::<PathBuf>())
        }
        _ => dir.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_without_scripts_has_empty_dirs() {
        let toml_str = r#"
            theme = "default"
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.scripts.dirs.is_empty());
    }

    #[test]
    fn pinned_defaults_empty_and_parses_entries() {
        let config: AppConfig = toml::from_str(r#"theme = "default""#).unwrap();
        assert!(config.pinned.is_empty());

        let config: AppConfig = toml::from_str(
            r#"
            theme = "default"
            pinned = ["Safari", "~/bin/quick.sh"]
            "#,
        )
        .unwrap();
        assert_eq!(config.pinned, vec!["Safari", "~/bin/quick.sh"]);
    }

    #[test]
    fn config_with_scripts_dirs_parses_paths() {
        let toml_str = r#"
            [scripts]
            dirs = ["/custom/scripts", "~/other_scripts"]
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.scripts.dirs,
            vec![
                PathBuf::from("/custom/scripts"),
                PathBuf::from("~/other_scripts")
            ]
        );
    }

    #[test]
    fn default_config_string_has_default_scripts_dir() {
        let config: AppConfig = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(
            config.scripts.dirs,
            vec![PathBuf::from("~/.config/aerofi/scripts")]
        );
    }

    #[test]
    fn reference_config_file_is_valid() {
        let content = include_str!("../../examples/config.toml");
        let config: AppConfig =
            toml::from_str(content).expect("examples/config.toml should parse cleanly");
        assert_eq!(config.theme, "tokyo-night");
        assert_eq!(config.bindings.toggle, "opt+space");
        assert_eq!(config.general.max_results, 20);
        assert!(config.sources.apps);
        assert!(config.sources.scripts);
        assert_eq!(config.scripts.dirs.len(), 2);
        assert_eq!(config.apps.ignore_names.len(), 5);
        assert_eq!(config.apps.ignore_dirs.len(), 1);
        assert!(config.apps.extra_dirs.is_empty());
        assert!(config.apps.extra_apps.is_empty());
        assert_eq!(config.aliases.len(), 4);
        assert_eq!(config.bindings.launcher.len(), 3);
        assert_eq!(config.bindings.global.len(), 2);
        assert_eq!(config.bindings.custom.len(), 3);
    }

    #[test]
    fn apps_config_parses_extra_dirs_and_apps() {
        let toml_str = r#"
            [apps]
            ignore_names = ["Test*"]
            extra_dirs = ["/opt/homebrew/Applications", "~/CustomApps"]
            extra_apps = ["/System/Library/CoreServices/Finder.app"]
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.apps.ignore_names, vec!["Test*"]);
        assert_eq!(config.apps.extra_dirs.len(), 2);
        assert_eq!(config.apps.extra_apps.len(), 1);

        let expanded_dirs = config.expanded_app_dirs();
        assert_eq!(expanded_dirs.len(), 2);
        assert!(!expanded_dirs[1].to_string_lossy().starts_with('~'));

        let expanded_apps = config.expanded_extra_apps();
        assert_eq!(expanded_apps.len(), 1);
        assert_eq!(
            expanded_apps[0],
            PathBuf::from("/System/Library/CoreServices/Finder.app")
        );
    }
}
