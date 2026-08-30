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

[general]
# Maximum number of results shown in the launcher list.
max_results = 20

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

[aliases]
# Alternate names for targets. Typing an alias finds the target in the
# search; typing it exactly runs the target immediately (no Enter), e.g.:
# "rc" = "Reload Configuration"

[shortcuts]
# Key combinations that run a target immediately while the launcher is
# open. Modifiers: cmd, ctrl, alt, shift (any order, before the key), e.g.:
# "cmd+r" = "Reload Configuration"

[global_shortcuts]
# System-wide shortcuts: run the named target directly, without opening the
# launcher. Use opt (or cmd) to avoid app conflicts, e.g.:
# "opt+d" = "Deploy"
# Registered at startup only (restart after editing); conflicting combos
# are skipped with a warning.
"#;

/// Launcher-wide behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Maximum number of results shown in the launcher list.
    pub max_results: usize,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { max_results: 20 }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptsConfig {
    /// Folders scanned for scripts. A leading `~` is expanded via
    /// [`AppConfig::expanded_script_dirs`].
    pub dirs: Vec<PathBuf>,
}

impl Default for ScriptsConfig {
    fn default() -> Self {
        Self {
            dirs: vec![PathBuf::from("~/.config/aerofi/scripts")],
        }
    }
}

/// Application source settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppsConfig {
    /// Names (or `*`/`?` patterns) of bundles hidden from the launcher.
    pub ignored: Vec<String>,
}

impl Default for AppsConfig {
    fn default() -> Self {
        Self {
            ignored: vec!["Uninstall*".to_string(), "Installer".to_string()],
        }
    }
}

/// Root configuration, stored as `~/.config/aerofi/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub sources: SourcesConfig,
    pub scripts: ScriptsConfig,
    pub apps: AppsConfig,
    /// Alternate names for targets (alias -> target name): used by the
    /// fuzzy search, and typing an alias exactly runs the target
    /// immediately (no Enter needed).
    pub aliases: HashMap<String, String>,
    /// Key combinations (e.g. "cmd+r" -> target name) that run the target
    /// immediately while the launcher is open.
    pub shortcuts: HashMap<String, String>,
    /// System-wide shortcuts (combo -> target name): run the target
    /// directly without opening the launcher. Registered at startup via
    /// Carbon `RegisterEventHotKey` (no Accessibility permission needed);
    /// see ADR 0002.
    pub global_shortcuts: HashMap<String, String>,
    /// Theme name resolved to `~/.config/aerofi/themes/{name}.toml`.
    /// The special value `"default"` uses the built-in Tokyo Night palette.
    pub theme: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            sources: SourcesConfig::default(),
            scripts: ScriptsConfig::default(),
            apps: AppsConfig::default(),
            aliases: HashMap::new(),
            shortcuts: HashMap::new(),
            global_shortcuts: HashMap::new(),
            theme: "default".to_string(),
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
            let defaults = AppConfig::default();
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
