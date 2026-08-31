//! The unified launcher element: an application, a shell script, or a
//! built-in action.
//!
//! [`Target`] is the single type the scanner produces, the search ranks and
//! the UI renders. Script metadata (`@raycast.*` comment tags) is extracted
//! in [`Target::script_from_file`].
//!
//! Supported Raycast annotations:
//! - `# @raycast.schemaVersion <version>` -> schema version (e.g. 1)
//! - `# @raycast.title <name>` -> display name
//! - `# @raycast.mode <mode>`  -> `silent` | `fullOutput` | `compact` | `inline` | `pipe`
//! - `# @raycast.packageName <pkg>` -> script package/group name
//! - `# @raycast.icon <icon>`  -> emoji or icon path/identifier
//! - `# @raycast.iconDark <icon>` -> dark mode icon path/identifier
//! - `# @raycast.refreshTime <time>` -> refresh interval (e.g. `1h`, `5m`)
//! - `# @raycast.needsConfirmation <bool>` -> confirmation before execution
//! - `# @raycast.argument1..3 <json/text>` -> typed positional input arguments
//! - `# @raycast.description <desc>` -> documentation description
//! - `# @raycast.author <author>` -> author name
//! - `# @raycast.authorURL <url>` -> author website/URL

use gpui::SharedString;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// Output mode of a script (from `@raycast.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptMode {
    Silent,
    FullOutput,
    Compact,
    Inline,
    Pipe,
}

impl ScriptMode {
    /// Canonical string form, as it appears in the annotation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::FullOutput => "fullOutput",
            Self::Compact => "compact",
            Self::Inline => "inline",
            Self::Pipe => "pipe",
        }
    }

    /// Parse a mode value. Unknown or missing values default to `FullOutput`.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "silent" => Self::Silent,
            "compact" => Self::Compact,
            "inline" => Self::Inline,
            "pipe" => Self::Pipe,
            _ => Self::FullOutput,
        }
    }
}

/// Option inside a dropdown argument in `@raycast.argument*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptArgumentOption {
    pub title: String,
    pub value: String,
}

/// Typed user input argument parsed from `@raycast.argument1`, `@raycast.argument2`, `@raycast.argument3`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptArgument {
    /// Input type: "text", "password", or "dropdown".
    #[serde(rename = "type", default)]
    pub arg_type: Option<String>,
    /// Placeholder text displayed in the input field.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Whether the argument is optional.
    #[serde(default)]
    pub optional: Option<bool>,
    /// Whether the argument should be percent-encoded when passed to the script.
    #[serde(rename = "percentEncoded", default)]
    pub percent_encoded: Option<bool>,
    /// Dropdown choices if `arg_type` is "dropdown".
    #[serde(default)]
    pub data: Option<Vec<ScriptArgumentOption>>,
}

impl ScriptArgument {
    /// Parse a JSON object or fallback to a plain placeholder string.
    pub fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        if trimmed.starts_with('{')
            && trimmed.ends_with('}')
            && let Ok(arg) = serde_json::from_str::<ScriptArgument>(trimmed)
        {
            return arg;
        }
        // Fallback: treat raw string as a text argument with the string as placeholder
        Self {
            arg_type: Some("text".to_string()),
            placeholder: Some(trimmed.to_string()),
            optional: None,
            percent_encoded: None,
            data: None,
        }
    }
}

/// Complete parsed Raycast Script Command metadata (`# @raycast.*`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RaycastMetadata {
    /// Schema version (from `@raycast.schemaVersion`, must be 1).
    pub schema_version: Option<u32>,
    /// Display title (from `@raycast.title`).
    pub title: Option<SharedString>,
    /// Execution output mode (from `@raycast.mode`).
    pub mode: Option<ScriptMode>,
    /// Package or group name (from `@raycast.packageName`).
    pub package_name: Option<SharedString>,
    /// Icon name, emoji, or path (from `@raycast.icon`).
    pub icon: Option<SharedString>,
    /// Dark mode icon path (from `@raycast.iconDark`).
    pub icon_dark: Option<SharedString>,
    /// Automatic update interval (from `@raycast.refreshTime`, e.g. "1h", "5m").
    pub refresh_time: Option<SharedString>,
    /// Whether to prompt before running (from `@raycast.needsConfirmation`).
    pub needs_confirmation: Option<bool>,
    /// Positional argument 1 (from `@raycast.argument1`).
    pub argument1: Option<ScriptArgument>,
    /// Positional argument 2 (from `@raycast.argument2`).
    pub argument2: Option<ScriptArgument>,
    /// Positional argument 3 (from `@raycast.argument3`).
    pub argument3: Option<ScriptArgument>,
    /// Documentation description (from `@raycast.description`).
    pub description: Option<SharedString>,
    /// Author name (from `@raycast.author`).
    pub author: Option<SharedString>,
    /// Author URL / website (from `@raycast.authorURL`).
    pub author_url: Option<SharedString>,
}

/// Built-in launcher actions: list entries with no on-disk target behind
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinAction {
    /// Re-read `config.toml` and rescan the target list.
    ReloadConfig,
}

/// AeroFi-specific script metatags parsed from `# @aerofi.*` annotations.
/// These temporarily override launcher UI settings while the script is the
/// active context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptMetatags {
    /// Hide the search input bar (`# @aerofi.show_search false`).
    pub show_search: Option<bool>,
    /// Override the number of list columns (`# @aerofi.columns N`).
    pub columns: Option<usize>,
}

/// A single launchable element: an application bundle, a shell script, or
/// a built-in action.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// An application bundle (`.app`).
    App {
        /// Display name (the bundle directory name without the `.app` suffix).
        name: SharedString,
        /// Path to the `.app` bundle on disk.
        path: Arc<Path>,
        /// Path to the cached icon file (TIFF), if extracted.
        icon_path: Option<Arc<Path>>,
    },
    /// A shell script plus its parsed `@raycast.*` and `@aerofi.*` metadata.
    Script {
        /// Display name (from `@raycast.title`, falling back to the file stem).
        name: SharedString,
        /// Path to the script on disk.
        path: Arc<Path>,
        /// Output mode (from `@raycast.mode`, defaulting to `FullOutput`).
        mode: ScriptMode,
        /// Icon (emoji or identifier) from `@raycast.icon`, if present.
        icon: Option<SharedString>,
        /// Parsed Raycast metadata tags.
        metadata: Arc<RaycastMetadata>,
        /// AeroFi metatags (`@aerofi.*`), if present.
        metatags: ScriptMetatags,
        /// Cached output for `inline` mode scripts (displayed as subtitle in the list).
        /// `None` means the script hasn't run yet or isn't inline mode.
        inline_output: Option<SharedString>,
    },
    /// A built-in action (e.g. reloading the configuration).
    Builtin {
        /// Display name.
        name: SharedString,
        /// The action to run.
        action: BuiltinAction,
    },
}

impl Target {
    /// The built-in "Reload Configuration" target.
    pub fn reload_config() -> Self {
        Self::Builtin {
            name: SharedString::from("Reload Configuration"),
            action: BuiltinAction::ReloadConfig,
        }
    }

    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            Self::App { name, .. } | Self::Script { name, .. } | Self::Builtin { name, .. } => {
                name.as_ref()
            }
        }
    }

    /// Stable identifier for history/frecency: the path on disk, or the
    /// display name for built-in actions.
    pub fn identifier(&self) -> SharedString {
        match self {
            Self::App { path, .. } | Self::Script { path, .. } => {
                SharedString::from(path.to_string_lossy().into_owned())
            }
            Self::Builtin { name, .. } => name.clone(),
        }
    }

    /// Text icon (emoji or identifier) for scripts; `None` for apps
    /// (they use a raster icon via [`icon_path`]) and built-in actions.
    pub fn icon(&self) -> Option<&str> {
        match self {
            Self::Script { icon, .. } => icon.as_deref(),
            Self::App { .. } | Self::Builtin { .. } => None,
        }
    }

    /// Path to the cached icon file for application bundles.
    /// Returns `None` for scripts and built-in actions.
    pub fn icon_path(&self) -> Option<&Path> {
        match self {
            Self::App { icon_path, .. } => icon_path.as_deref(),
            Self::Script { .. } | Self::Builtin { .. } => None,
        }
    }

    /// AeroFi metatags for scripts. Returns `None` for apps and built-ins.
    pub fn metatags(&self) -> Option<&ScriptMetatags> {
        match self {
            Self::Script { metatags, .. } => Some(metatags),
            Self::App { .. } | Self::Builtin { .. } => None,
        }
    }

    /// Parsed Raycast metadata for scripts. Returns `None` for apps and built-ins.
    #[allow(dead_code)]
    pub fn metadata(&self) -> Option<&RaycastMetadata> {
        match self {
            Self::Script { metadata, .. } => Some(metadata),
            Self::App { .. } | Self::Builtin { .. } => None,
        }
    }

    /// Cached inline output for scripts with `mode = inline`.
    pub fn inline_output(&self) -> Option<&str> {
        match self {
            Self::Script { inline_output, .. } => inline_output.as_deref(),
            _ => None,
        }
    }

    /// Set the cached inline output. Only meaningful for `mode = inline` scripts.
    pub fn set_inline_output(&mut self, output: Option<SharedString>) {
        if let Self::Script { inline_output, .. } = self {
            *inline_output = output;
        }
    }

    /// The `refreshTime` string from metadata (e.g. "5m", "1h").
    #[allow(dead_code)]
    pub fn refresh_time(&self) -> Option<&str> {
        match self {
            Self::Script { metadata, .. } => metadata.refresh_time.as_deref(),
            _ => None,
        }
    }

    /// The `packageName` string from metadata, displayed as subtitle.
    pub fn package_name(&self) -> Option<&str> {
        match self {
            Self::Script { metadata, .. } => metadata.package_name.as_deref(),
            _ => None,
        }
    }

    /// Whether the script requires confirmation before running.
    #[allow(dead_code)]
    pub fn needs_confirmation(&self) -> bool {
        match self {
            Self::Script { metadata, .. } => metadata.needs_confirmation.unwrap_or(false),
            _ => false,
        }
    }

    /// Dark mode icon path (from `@raycast.iconDark`).
    #[allow(dead_code)]
    pub fn icon_dark(&self) -> Option<&str> {
        match self {
            Self::Script { metadata, .. } => metadata.icon_dark.as_deref(),
            _ => None,
        }
    }

    /// Extract all defined arguments from the script.
    #[allow(dead_code)]
    pub fn arguments(&self) -> Vec<&ScriptArgument> {
        match self {
            Self::Script { metadata, .. } => {
                let mut args = Vec::new();
                if let Some(a1) = &metadata.argument1 {
                    args.push(a1);
                }
                if let Some(a2) = &metadata.argument2 {
                    args.push(a2);
                }
                if let Some(a3) = &metadata.argument3 {
                    args.push(a3);
                }
                args
            }
            _ => Vec::new(),
        }
    }

    /// Parse a single script file into a `Target::Script`.
    pub fn script_from_file(path: &Path) -> Option<Self> {
        let file_stem = path.file_stem()?.to_string_lossy().into_owned();
        let content = std::fs::read_to_string(path).ok()?;

        let mut metadata = RaycastMetadata::default();
        let mut metatags = ScriptMetatags::default();

        // Annotations live in the comment lines at the beginning of the script.
        for line in content.lines().take(50) {
            let trimmed = line.trim();
            // Skip shebang or empty lines
            if trimmed.is_empty() || trimmed.starts_with("#!") {
                continue;
            }
            // Keep only `# ...` comment lines.
            let Some(rest) = trimmed.strip_prefix('#') else {
                // If we encounter a non-comment line, stop scanning annotations
                break;
            };
            let comment = rest.trim_start();

            // `@aerofi.*` annotations (AeroFi-specific metatags).
            if let Some(annotation) = comment.strip_prefix("@aerofi.") {
                if let Some((field, value)) = annotation.split_once(|c: char| c.is_whitespace()) {
                    let value = value.trim();
                    match field.trim() {
                        "show_search" if metatags.show_search.is_none() => {
                            metatags.show_search = Some(value != "false");
                        }
                        "columns" if metatags.columns.is_none() => {
                            if let Ok(n) = value.parse::<usize>() {
                                metatags.columns = Some(n);
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }

            // `@raycast.*` annotations (Raycast-compatible metadata).
            if let Some(annotation) = comment.strip_prefix("@raycast.") {
                let Some((field, value)) = annotation.split_once(|c: char| c.is_whitespace())
                else {
                    continue;
                };
                let value = value.trim();
                if value.is_empty() {
                    continue;
                }
                match field.trim() {
                    "schemaVersion" if metadata.schema_version.is_none() => {
                        metadata.schema_version = value.parse::<u32>().ok();
                    }
                    "title" if metadata.title.is_none() => {
                        metadata.title = Some(SharedString::from(value.to_string()));
                    }
                    "mode" if metadata.mode.is_none() => {
                        metadata.mode = Some(ScriptMode::parse(value));
                    }
                    "packageName" if metadata.package_name.is_none() => {
                        metadata.package_name = Some(SharedString::from(value.to_string()));
                    }
                    "icon" if metadata.icon.is_none() => {
                        metadata.icon = Some(SharedString::from(value.to_string()));
                    }
                    "iconDark" if metadata.icon_dark.is_none() => {
                        metadata.icon_dark = Some(SharedString::from(value.to_string()));
                    }
                    "refreshTime" if metadata.refresh_time.is_none() => {
                        metadata.refresh_time = Some(SharedString::from(value.to_string()));
                    }
                    "needsConfirmation" if metadata.needs_confirmation.is_none() => {
                        metadata.needs_confirmation = Some(value.eq_ignore_ascii_case("true"));
                    }
                    "argument1" if metadata.argument1.is_none() => {
                        metadata.argument1 = Some(ScriptArgument::parse(value));
                    }
                    "argument2" if metadata.argument2.is_none() => {
                        metadata.argument2 = Some(ScriptArgument::parse(value));
                    }
                    "argument3" if metadata.argument3.is_none() => {
                        metadata.argument3 = Some(ScriptArgument::parse(value));
                    }
                    "description" if metadata.description.is_none() => {
                        metadata.description = Some(SharedString::from(value.to_string()));
                    }
                    "author" if metadata.author.is_none() => {
                        metadata.author = Some(SharedString::from(value.to_string()));
                    }
                    "authorURL" if metadata.author_url.is_none() => {
                        metadata.author_url = Some(SharedString::from(value.to_string()));
                    }
                    _ => {}
                }
            }
        }

        let name = metadata
            .title
            .clone()
            .unwrap_or_else(|| SharedString::from(file_stem));
        let mode = metadata.mode.unwrap_or(ScriptMode::FullOutput);
        let icon = metadata.icon.clone();

        Some(Self::Script {
            name,
            mode,
            icon,
            path: Arc::from(path),
            metadata: Arc::new(metadata),
            metatags,
            inline_output: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_all_raycast_tags() {
        let dir = std::env::temp_dir();
        let file_path = dir.join(format!("test_raycast_{}.sh", std::process::id()));
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(
            file,
            r#"#!/bin/bash

# Required parameters:
# @raycast.schemaVersion 1
# @raycast.title GitHub Repository Stars
# @raycast.mode compact

# Optional parameters:
# @raycast.packageName Developer Utilities
# @raycast.icon ⭐️
# @raycast.iconDark 🌟
# @raycast.refreshTime 5m
# @raycast.needsConfirmation true
# @raycast.argument1 {{ "type": "text", "placeholder": "owner/repo", "optional": false, "percentEncoded": true }}
# @raycast.argument2 {{ "type": "dropdown", "placeholder": "Branch", "data": [{{"title": "Main", "value": "main"}}, {{"title": "Dev", "value": "dev"}}] }}
# @raycast.argument3 plain_argument_placeholder

# Documentation:
# @raycast.description Show GitHub star count for a repository
# @raycast.author Timur Iskakov
# @raycast.authorURL https://github.com/frostymur

# @aerofi.show_search false
# @aerofi.columns 3

echo "Running script..."
"#
        )
        .unwrap();

        let target = Target::script_from_file(&file_path).expect("Failed to parse script");
        let Target::Script {
            name,
            mode,
            icon,
            metadata,
            metatags,
            ..
        } = target
        else {
            panic!("Expected Target::Script");
        };

        assert_eq!(name.as_ref(), "GitHub Repository Stars");
        assert_eq!(mode, ScriptMode::Compact);
        assert_eq!(icon.as_deref(), Some("⭐️"));

        assert_eq!(metadata.schema_version, Some(1));
        assert_eq!(metadata.title.as_deref(), Some("GitHub Repository Stars"));
        assert_eq!(metadata.mode, Some(ScriptMode::Compact));
        assert_eq!(
            metadata.package_name.as_deref(),
            Some("Developer Utilities")
        );
        assert_eq!(metadata.icon.as_deref(), Some("⭐️"));
        assert_eq!(metadata.icon_dark.as_deref(), Some("🌟"));
        assert_eq!(metadata.refresh_time.as_deref(), Some("5m"));
        assert_eq!(metadata.needs_confirmation, Some(true));

        // Argument 1
        let arg1 = metadata.argument1.as_ref().expect("arg1 missing");
        assert_eq!(arg1.arg_type.as_deref(), Some("text"));
        assert_eq!(arg1.placeholder.as_deref(), Some("owner/repo"));
        assert_eq!(arg1.optional, Some(false));
        assert_eq!(arg1.percent_encoded, Some(true));

        // Argument 2
        let arg2 = metadata.argument2.as_ref().expect("arg2 missing");
        assert_eq!(arg2.arg_type.as_deref(), Some("dropdown"));
        assert_eq!(arg2.placeholder.as_deref(), Some("Branch"));
        let data = arg2.data.as_ref().expect("arg2 dropdown data missing");
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].title, "Main");
        assert_eq!(data[0].value, "main");

        // Argument 3 (fallback plain string)
        let arg3 = metadata.argument3.as_ref().expect("arg3 missing");
        assert_eq!(arg3.arg_type.as_deref(), Some("text"));
        assert_eq!(
            arg3.placeholder.as_deref(),
            Some("plain_argument_placeholder")
        );

        // Documentation
        assert_eq!(
            metadata.description.as_deref(),
            Some("Show GitHub star count for a repository")
        );
        assert_eq!(metadata.author.as_deref(), Some("Timur Iskakov"));
        assert_eq!(
            metadata.author_url.as_deref(),
            Some("https://github.com/frostymur")
        );

        // Metatags
        assert_eq!(metatags.show_search, Some(false));
        assert_eq!(metatags.columns, Some(3));

        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn parses_all_script_modes() {
        assert_eq!(ScriptMode::parse("silent"), ScriptMode::Silent);
        assert_eq!(ScriptMode::parse("fullOutput"), ScriptMode::FullOutput);
        assert_eq!(ScriptMode::parse("compact"), ScriptMode::Compact);
        assert_eq!(ScriptMode::parse("inline"), ScriptMode::Inline);
        assert_eq!(ScriptMode::parse("pipe"), ScriptMode::Pipe);
        assert_eq!(ScriptMode::parse("unknown_mode"), ScriptMode::FullOutput);
    }
}
