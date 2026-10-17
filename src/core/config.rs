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
# How shift+enter launches a new window for a running app:
# "smart" (default): asks the app itself to open a new window in its
# existing process via AppleScript (no duplicate instance), falling back
# to `open -n` for apps without a scripting dictionary (e.g. Chrome); the
# AppleScript path always activates the app
# "force": always launches a fresh instance via `open -n`
# "background": `open -n -g` — the window appears on the current workspace
# without activating the app (handy with tiling window managers)
new_instance_mode = "smart"
# How the filter query matches target names/aliases:
# "fuzzy" (default, fzf-style), "prefix" (must start with the query),
# or "glob" (* = any run of characters, ? = one character).
matching = "fuzzy"
# How results are ordered: "frecency" (default: fuzzy score + usage history)
# or "lexical" (alphabetical; pinned items still lead).
ranking = "frecency"
# Watch the application, script, and plugin directories for changes so
# new/removed items appear without a reload (applies after a restart).
# A source disabled in [sources] is never watched.
watch_sources = true

[sources]
# Which target sources the launcher indexes.
apps = true
scripts = true
system_settings = true

# Named runtime source filters. Each mode is a comma-separated source
# list: "apps", "scripts", "builtins", "all". While a mode is active, the
# launcher lists only those sources. Any loaded plugin name is also a
# mode: it enters the plugin's session with its prefix applied and
# hidden. Cycle modes with Cmd+. or bind them to buttons:
# action = "mode:scripts". `mode:all` (or Cmd+.) switches back to
# every source; a mode you named `all` in any case wins over the reset.
# Unknown source names are skipped with a warning.
# [modes]
# scripts = "scripts,builtins"

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
# A plugin name opens the launcher directly in that plugin's mode (the
# prefix is pre-filled, no typing needed), e.g.:
# "opt+f" = "file-search"
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

/// How Shift+Enter launches a new instance of a running app.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewInstanceMode {
    /// Ask the app itself to open a new window via AppleScript (same
    /// process, no duplicate instance), falling back to `open -n` for apps
    /// without a scripting dictionary (e.g. Chrome). The AppleScript path
    /// always activates the app.
    #[default]
    Smart,
    /// Always launch a fresh instance via `open -n`.
    Force,
    /// Launch a fresh instance via `open -n -g`: the window appears on the
    /// current workspace without activating the app (handy with tiling
    /// window managers). Uses a new process.
    Background,
}

/// Launcher-wide behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// The maximum number of search results displayed in the UI.
    pub max_results: usize,
    /// How Shift+Enter launches a new instance of a running app: `smart`
    /// (default) asks the app to open a new window in its existing process
    /// via AppleScript and falls back to `open -n`; `force` always uses
    /// `open -n`; `background` uses `open -n -g` so the window lands on the
    /// current workspace without activating the app.
    pub new_instance_mode: NewInstanceMode,
    /// Filter query matching mode: `fuzzy` (default), `prefix`, or `glob`.
    pub matching: MatchMode,
    /// Result ranking: `frecency` (default) or `lexical` (alphabetical).
    pub ranking: RankingMode,
    /// Watch the application, script, and plugin directories for changes
    /// so new/removed items appear without a manual reload (Cmd+R).
    pub watch_sources: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            max_results: 20,
            new_instance_mode: NewInstanceMode::Smart,
            matching: MatchMode::Fuzzy,
            ranking: RankingMode::Frecency,
            watch_sources: true,
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
    /// Named runtime source filters (`[modes]`), switched at runtime with
    /// Cmd+. or a `mode:<name>` button action.
    /// Keys are mode names; values are comma-separated source lists
    /// (`"apps,scripts,builtins,all"`). A loaded plugin name is also a
    /// valid mode (it enters the plugin's session). `BTreeMap` so the
    /// Cmd+. cycle order is deterministic.
    pub modes: std::collections::BTreeMap<String, String>,
    /// Per-target action menus (Cmd+K): `[actions.apps]` /
    /// `[actions.scripts]`.
    #[serde(default)]
    pub actions: Option<ActionsConfig>,
    /// Plugins allowed as `Cmd+.` cycle stops, lifted from the reserved
    /// `[modes] plugins` key by [`AppConfig::sanitize`]. `None` = every
    /// loaded plugin cycles (key absent or `= "all"`); `Some(names)` =
    /// only the named plugins (empty list = no plugin stops).
    #[serde(skip)]
    pub cycle_plugins: Option<Vec<String>>,
}

/// Built-in action toggles plus custom actions for one target kind.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionsConfig {
    /// Actions offered for application rows.
    #[serde(default)]
    pub apps: Option<ActionSetConfig>,
    /// Actions offered for script rows.
    #[serde(default)]
    pub scripts: Option<ActionSetConfig>,
}

/// One target kind's action menu: the built-ins (each defaulting to on)
/// followed by the custom entries in file order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSetConfig {
    /// "Reveal in Finder" (`open -R`).
    #[serde(default = "default_true")]
    pub reveal: bool,
    /// "Copy Path" — copies the target's full path to the clipboard.
    #[serde(default = "default_true")]
    pub copy_path: bool,
    /// "New Window" (smart launch) — application rows only.
    #[serde(default = "default_true")]
    pub new_window: bool,
    /// Extra shell actions; `{path}` and `{name}` are replaced with the
    /// target's path and display name before running. Application menus
    /// default `new_window` on; script menus never offer it (the section
    /// is parsed identically — see `Target::actions`).
    #[serde(default)]
    pub custom: Vec<CustomActionConfig>,
}

impl Default for ActionSetConfig {
    /// No-config default: every built-in offered. (A derived `Default`
    /// would be all-`false` — `bool::default()` — which is wrong here;
    /// serde's `default_true` only covers TOML parsing, not this impl.)
    fn default() -> Self {
        Self {
            reveal: true,
            copy_path: true,
            new_window: true,
            custom: Vec::new(),
        }
    }
}

/// A user-defined action-menu entry (`[[actions.apps.custom]]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomActionConfig {
    /// Label shown in the menu.
    pub label: String,
    /// Shell command, run detached via `sh -c`; `{path}` / `{name}`
    /// template variables are substituted at menu-open time.
    pub command: String,
}

fn default_true() -> bool {
    true
}

/// Parse the reserved `[modes] plugins` value: comma-separated plugin
/// names. `all` (case-insensitive, even alongside names) means "every
/// loaded plugin" (`None`); an empty value means "no plugin stops"
/// (`Some([])`).
pub fn parse_cycle_plugins(raw: &str) -> Option<Vec<String>> {
    let names: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if names.iter().any(|n| n.eq_ignore_ascii_case("all")) {
        None
    } else {
        Some(names)
    }
}

/// The next entry of the `Cmd+.` mode cycle: the `[modes]` keys in the
/// order given (BTreeMap = alphabetical today) followed by the loaded
/// plugin names. `current = None` starts the cycle at the first entry;
/// `None` as the result means "wrap back to All". The lookup happens in
/// the *concatenated* list — a config mode and a plugin mode are one
/// sequence, so the last config mode hands off to the first plugin.
pub fn next_mode_cycle_entry(
    config_modes: &[String],
    plugin_modes: &[String],
    current: Option<&str>,
) -> Option<String> {
    let all: Vec<&String> = config_modes.iter().chain(plugin_modes.iter()).collect();
    match current {
        None => all.first().cloned().cloned(),
        Some(c) => all
            .iter()
            .position(|n| n.as_str() == c)
            .and_then(|p| all.get(p + 1))
            .cloned()
            .cloned(),
    }
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
            modes: std::collections::BTreeMap::new(),
            actions: None,
            cycle_plugins: None,
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

        config.sanitize()
    }

    /// Post-parse normalization: lifts the reserved `[modes] plugins`
    /// key out of the modes map. It must happen before the launcher
    /// validates modes — as a "mode" it would be a dead filter with a
    /// bogus warning, because `plugins` is not a source name.
    pub fn sanitize(mut self) -> Self {
        if let Some(raw) = self.modes.remove("plugins") {
            self.cycle_plugins = parse_cycle_plugins(&raw);
        }
        self
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
        assert_eq!(config.general.new_instance_mode, NewInstanceMode::Smart);
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

    #[test]
    fn new_instance_mode_parses_all_values() {
        let c: AppConfig = toml::from_str(
            r#"theme = "default"
            [general]
            new_instance_mode = "force"
            "#,
        )
        .unwrap();
        assert_eq!(c.general.new_instance_mode, NewInstanceMode::Force);

        let c: AppConfig = toml::from_str(
            r#"theme = "default"
            [general]
            new_instance_mode = "background"
            "#,
        )
        .unwrap();
        assert_eq!(c.general.new_instance_mode, NewInstanceMode::Background);
    }

    #[test]
    fn modes_section_parses_and_defaults_empty() {
        let c: AppConfig = toml::from_str(
            r#"
            theme = "default"
            [general]
            [modes]
            scripts = "scripts,builtins"
            dev = "apps, scripts"
            "#,
        )
        .unwrap();
        assert_eq!(c.modes.len(), 2);
        assert_eq!(
            c.modes.get("scripts"),
            Some(&"scripts,builtins".to_string())
        );

        // No [modes] section at all: empty, old configs keep parsing.
        let c: AppConfig = toml::from_str(r#"theme = "default""#).unwrap();
        assert!(c.modes.is_empty());
    }

    #[test]
    fn actions_section_parses_and_defaults() {
        let c: AppConfig = toml::from_str(
            r#"
            theme = "default"
            [actions.apps]
            copy_path = false
            [[actions.apps.custom]]
            label = "Open in Terminal"
            command = "open -a Terminal {path}"
            [actions.scripts]
            reveal = false
            "#,
        )
        .unwrap();
        let apps = c.actions.as_ref().and_then(|a| a.apps.as_ref()).unwrap();
        assert!(apps.reveal, "untouched built-ins default on");
        assert!(!apps.copy_path);
        assert!(apps.new_window);
        assert_eq!(apps.custom.len(), 1);
        assert_eq!(apps.custom[0].command, "open -a Terminal {path}");
        let scripts = c.actions.as_ref().and_then(|a| a.scripts.as_ref()).unwrap();
        assert!(!scripts.reveal);
        assert!(scripts.copy_path);

        // No [actions] section at all: None (the code defaults kick in).
        let c: AppConfig = toml::from_str(r#"theme = "default""#).unwrap();
        assert!(c.actions.is_none());
    }

    #[test]
    fn mode_cycle_walks_config_modes_then_plugins() {
        use crate::core::config::next_mode_cycle_entry;
        let modes: Vec<String> = ["apps", "builtins", "dev", "scripts"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let plugins: Vec<String> = ["file-search", "web-search"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // The regression this helper pins: the last *config* mode hands
        // off to the first plugin instead of wrapping to All.
        assert_eq!(
            next_mode_cycle_entry(&modes, &plugins, None).as_deref(),
            Some("apps")
        );
        assert_eq!(
            next_mode_cycle_entry(&modes, &plugins, Some("apps")).as_deref(),
            Some("builtins")
        );
        assert_eq!(
            next_mode_cycle_entry(&modes, &plugins, Some("dev")).as_deref(),
            Some("scripts")
        );
        assert_eq!(
            next_mode_cycle_entry(&modes, &plugins, Some("scripts")).as_deref(),
            Some("file-search")
        );
        assert_eq!(
            next_mode_cycle_entry(&modes, &plugins, Some("file-search")).as_deref(),
            Some("web-search")
        );
        assert_eq!(
            next_mode_cycle_entry(&modes, &plugins, Some("web-search")),
            None
        );
        // No plugins: last mode wraps to All.
        assert_eq!(next_mode_cycle_entry(&modes, &[], Some("scripts")), None);
        // No config modes: plugins alone.
        let none: Vec<String> = Vec::new();
        assert_eq!(
            next_mode_cycle_entry(&none, &plugins, None).as_deref(),
            Some("file-search")
        );
        assert_eq!(
            next_mode_cycle_entry(&none, &plugins, Some("file-search")).as_deref(),
            Some("web-search")
        );
    }

    #[test]
    fn modes_plugins_key_is_lifted_into_cycle_gate() {
        // The reserved `[modes] plugins` key selects which loaded
        // plugins cycle; it must leave the modes map entirely (it is
        // not a mode and must not be validated as one).
        let toml_str = "\
theme = \"default\"\n\
[modes]\n\
apps = \"apps\"\n\
plugins = \"file-search\"\n";
        let mut c: AppConfig = toml::from_str(toml_str).unwrap();
        c = c.sanitize();
        assert!(!c.modes.contains_key("plugins"));
        assert_eq!(c.modes.len(), 1);
        assert_eq!(c.cycle_plugins, Some(vec!["file-search".to_string()]));

        // "all" (any case) means every loaded plugin cycles -> None.
        let mut c: AppConfig =
            toml::from_str("theme = \"default\"\n[modes]\nplugins = \"All\"\n").unwrap();
        c = c.sanitize();
        assert_eq!(c.cycle_plugins, None);

        // Empty value = no plugin stops (Some([]) — distinct from None).
        let mut c: AppConfig =
            toml::from_str("theme = \"default\"\n[modes]\nplugins = \"\"\n").unwrap();
        c = c.sanitize();
        assert_eq!(c.cycle_plugins, Some(Vec::new()));

        // No key at all -> None (every loaded plugin cycles).
        let c: AppConfig = toml::from_str("theme = \"default\"").unwrap();
        assert_eq!(c.sanitize().cycle_plugins, None);

        // Multiple names keep file order.
        let mut c: AppConfig =
            toml::from_str("theme = \"default\"\n[modes]\nplugins = \"web-search, file-search\"")
                .unwrap();
        c = c.sanitize();
        assert_eq!(
            c.cycle_plugins,
            Some(vec!["web-search".to_string(), "file-search".to_string()])
        );
    }
}
