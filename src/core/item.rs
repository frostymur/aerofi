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
//! - `# @raycast.mode <mode>`  -> `silent` | `fullOutput` | `compact` | `inline` | `pipe` | `gui`
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
use std::path::{Path, PathBuf};
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
    Gui,
}

impl ScriptMode {
    /// Canonical string form, as it appears in the annotation.
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::FullOutput => "fullOutput",
            Self::Compact => "compact",
            Self::Inline => "inline",
            Self::Pipe => "pipe",
            Self::Gui => "gui",
        }
    }

    /// Parse a mode value. Unknown or missing values default to `FullOutput`.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "silent" => Self::Silent,
            "compact" => Self::Compact,
            "inline" => Self::Inline,
            "pipe" => Self::Pipe,
            "gui" => Self::Gui,
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

/// aerofi-specific script metatags parsed from `# @aerofi.*` annotations.
/// Committed when the script is executed; stay active until the launcher
/// is hidden. Selecting the script never applies them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptMetatags {
    /// Hide the search input bar (`# @aerofi.show_search false`).
    pub show_search: Option<bool>,
    /// Override the number of list columns (`# @aerofi.columns N`).
    pub columns: Option<usize>,
    /// Mode name for theme overrides (`# @aerofi.preset emoji`).
    pub layout: Option<String>,
    /// Override window width in points (`# @aerofi.width 320`).
    pub width: Option<f32>,
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
        /// Pre-resolved path to the icon image (only when `icon` is an image
        /// path). Computed once at parse time so per-frame rendering never
        /// re-runs tilde/dir resolution. `None` for emoji/text icons.
        icon_image_path: Option<Arc<Path>>,
        /// Parsed Raycast metadata tags.
        metadata: Arc<RaycastMetadata>,
        /// aerofi metatags (`@aerofi.*`), if present.
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
        /// Optional icon (e.g. emoji or symbol).
        icon: Option<SharedString>,
    },
    /// An item returned by a dynamic `.dylib` plugin.
    PluginItem {
        /// Display name.
        name: SharedString,
        /// Secondary descriptive text.
        subtitle: Option<SharedString>,
        /// Optional icon identifier.
        icon: Option<SharedString>,
        /// The unique ID generated by the plugin to identify this item.
        plugin_id: SharedString,
        /// The internal name of the plugin that generated this item.
        plugin_name: SharedString,
    },
}

impl Target {
    /// The built-in "Reload Configuration" target.
    pub fn reload_config() -> Self {
        Self::Builtin {
            name: SharedString::from("Reload Configuration"),
            action: BuiltinAction::ReloadConfig,
            icon: None,
        }
    }

    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            Self::App { name, .. }
            | Self::Script { name, .. }
            | Self::Builtin { name, .. }
            | Self::PluginItem { name, .. } => name.as_ref(),
        }
    }

    /// Stable identifier for history/frecency: the path on disk, or the
    /// display name for built-in actions.
    pub fn identifier(&self) -> &str {
        match self {
            Self::App { path, .. } | Self::Script { path, .. } => path.to_str().unwrap_or(""),
            Self::Builtin { name, .. } => name.as_ref(),
            Self::PluginItem { plugin_id, .. } => plugin_id.as_ref(),
        }
    }

    /// Text icon (emoji or identifier) for scripts and built-ins; `None` for apps
    /// (they use a raster icon via [`icon_path`]).
    pub fn icon(&self) -> Option<&str> {
        match self {
            Self::Script { icon, .. } => icon.as_deref(),
            Self::Builtin { icon, .. } => icon.as_deref(),
            Self::PluginItem { icon, .. } => icon.as_deref(),
            Self::App { .. } => None,
        }
    }

    /// Path to the cached icon file for application bundles.
    /// Returns `None` for scripts and built-in actions.
    pub fn icon_path(&self) -> Option<&Path> {
        match self {
            Self::App { icon_path, .. } => icon_path.as_deref(),
            Self::Script { .. } | Self::Builtin { .. } | Self::PluginItem { .. } => None,
        }
    }

    /// Pre-resolved path to a script's image icon (from `@raycast.icon`),
    /// or `None` when the icon is an emoji/text glyph (or not a script).
    pub fn icon_image_path(&self) -> Option<&Path> {
        match self {
            Self::Script {
                icon_image_path, ..
            } => icon_image_path.as_deref(),
            _ => None,
        }
    }

    /// aerofi metatags for scripts. Returns `None` for apps and built-ins.
    pub fn metatags(&self) -> Option<&ScriptMetatags> {
        match self {
            Self::Script { metatags, .. } => Some(metatags),
            Self::App { .. } | Self::Builtin { .. } | Self::PluginItem { .. } => None,
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
            Self::PluginItem { subtitle, .. } => subtitle.as_deref(),
            _ => None,
        }
    }

    /// Whether the script requires confirmation before running.
    pub fn needs_confirmation(&self) -> bool {
        match self {
            Self::Script { metadata, .. } => metadata.needs_confirmation.unwrap_or(false),
            _ => false,
        }
    }

    /// Extract all defined arguments from the script.
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

    /// Category label for the right-side row badge: "Script", "Application",
    /// or "Aerofi".
    pub fn category_label(&self) -> &'static str {
        match self {
            Self::Script { .. } => "Script",
            Self::App { .. } => "Application",
            Self::Builtin { .. } => "Aerofi",
            Self::PluginItem { .. } => {
                // Return a static string representation of the plugin name if possible,
                // but since we need &'static str and plugin_name is dynamic, we just return "Plugin"
                "Plugin"
            }
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
            // Skip shebang, empty lines, or multi-line comment boundaries
            if trimmed.is_empty()
                || trimmed.starts_with("#!")
                || trimmed == "\"\"\""
                || trimmed == "'''"
                || trimmed == "/*"
                || trimmed == "*/"
            {
                continue;
            }
            // Keep only comment lines (`#`, `//`, or `--`).
            let rest = if let Some(r) = trimmed.strip_prefix('#') {
                r
            } else if let Some(r) = trimmed.strip_prefix("//") {
                r
            } else if let Some(r) = trimmed.strip_prefix("--") {
                r
            } else {
                // If we encounter actual code, stop scanning annotations
                break;
            };
            let comment = rest.trim_start();

            // Support both `@aerofi.` and `@raycast.` annotation prefixes interchangeably.
            let annotation_opt = comment
                .strip_prefix("@aerofi.")
                .map(|a| (a, true))
                .or_else(|| comment.strip_prefix("@raycast.").map(|a| (a, false)));

            if let Some((annotation, is_aerofi)) = annotation_opt {
                let Some((field, value)) = annotation.split_once(|c: char| c.is_whitespace())
                else {
                    continue;
                };
                let value = value.trim();
                if value.is_empty() {
                    continue;
                }
                match field.trim() {
                    // aerofi-specific metatags
                    "show_search" if is_aerofi || metatags.show_search.is_none() => {
                        metatags.show_search = Some(value != "false");
                    }
                    "columns" if is_aerofi || metatags.columns.is_none() => {
                        if let Ok(n) = value.parse::<usize>() {
                            metatags.columns = Some(n);
                        }
                    }
                    "preset" if is_aerofi || metatags.layout.is_none() => {
                        metatags.layout = Some(value.to_string());
                    }
                    "width" if is_aerofi || metatags.width.is_none() => {
                        if let Ok(w) = value.parse::<f32>() {
                            metatags.width = Some(w);
                        }
                    }
                    // Metadata annotations (supported via @aerofi.* and @raycast.*)
                    "schemaVersion" if is_aerofi || metadata.schema_version.is_none() => {
                        metadata.schema_version = value.parse::<u32>().ok();
                    }
                    "title" if is_aerofi || metadata.title.is_none() => {
                        metadata.title = Some(SharedString::from(value.to_string()));
                    }
                    "mode" if is_aerofi || metadata.mode.is_none() => {
                        metadata.mode = Some(ScriptMode::parse(value));
                    }
                    "packageName" if is_aerofi || metadata.package_name.is_none() => {
                        metadata.package_name = Some(SharedString::from(value.to_string()));
                    }
                    "icon" if is_aerofi || metadata.icon.is_none() => {
                        metadata.icon = Some(SharedString::from(value.to_string()));
                    }
                    "iconDark" if is_aerofi || metadata.icon_dark.is_none() => {
                        metadata.icon_dark = Some(SharedString::from(value.to_string()));
                    }
                    "refreshTime" if is_aerofi || metadata.refresh_time.is_none() => {
                        metadata.refresh_time = Some(SharedString::from(value.to_string()));
                    }
                    "needsConfirmation" if is_aerofi || metadata.needs_confirmation.is_none() => {
                        metadata.needs_confirmation = Some(value.eq_ignore_ascii_case("true"));
                    }
                    "argument1" if is_aerofi || metadata.argument1.is_none() => {
                        metadata.argument1 = Some(ScriptArgument::parse(value));
                    }
                    "argument2" if is_aerofi || metadata.argument2.is_none() => {
                        metadata.argument2 = Some(ScriptArgument::parse(value));
                    }
                    "argument3" if is_aerofi || metadata.argument3.is_none() => {
                        metadata.argument3 = Some(ScriptArgument::parse(value));
                    }
                    "description" if is_aerofi || metadata.description.is_none() => {
                        metadata.description = Some(SharedString::from(value.to_string()));
                    }
                    "author" if is_aerofi || metadata.author.is_none() => {
                        metadata.author = Some(SharedString::from(value.to_string()));
                    }
                    "authorURL" if is_aerofi || metadata.author_url.is_none() => {
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
        let icon_image_path = Self::resolve_icon_image(icon.as_deref(), path);

        Some(Self::Script {
            name,
            mode,
            icon,
            icon_image_path,
            path: Arc::from(path),
            metadata: Arc::new(metadata),
            metatags,
            inline_output: None,
        })
    }

    /// If `icon` is a local image path, resolve it to an absolute path once
    /// (expanding a leading `~` and joining bare relative paths onto the
    /// script's directory). Returns `None` for emoji/glyph icons so callers
    /// fall back to text rendering.
    fn resolve_icon_image(icon: Option<&str>, script_path: &Path) -> Option<Arc<Path>> {
        let icon = icon?;
        if !Self::looks_like_image(icon) {
            return None;
        }
        let resolved = if icon.starts_with('~') || icon.starts_with('/') {
            Self::expand_home(icon)
        } else {
            script_path
                .parent()
                .map(|d| d.join(icon))
                .unwrap_or_else(|| Path::new(icon).to_path_buf())
        };
        Some(Arc::from(resolved))
    }

    /// Whether a string looks like a local image file path (same rules the
    /// UI uses to decide between an icon image and a text glyph).
    fn looks_like_image(s: &str) -> bool {
        s.starts_with('/')
            || s.starts_with("./")
            || s.ends_with(".png")
            || s.ends_with(".jpg")
            || s.ends_with(".jpeg")
            || s.ends_with(".webp")
            || s.ends_with(".tiff")
    }

    /// Expand a leading `~` to the user's home directory.
    fn expand_home(p: &str) -> PathBuf {
        if let Some(rest) = p.strip_prefix('~')
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest);
        }
        Path::new(p).to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

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
# @aerofi.preset list

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
        assert_eq!(metatags.layout.as_deref(), Some("list"));

        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn parses_all_aerofi_tags() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("test_aerofi_tags_{}.sh", std::process::id()));

        std::fs::write(
            &file_path,
            r#"#!/usr/bin/env bash
# @aerofi.schemaVersion 1
# @aerofi.title Theme Switcher
# @aerofi.mode gui
# @aerofi.packageName aerofi Utilities
# @aerofi.icon 🎨
# @aerofi.iconDark 🎭
# @aerofi.refreshTime 10m
# @aerofi.needsConfirmation false
# @aerofi.argument1 {"type": "text", "placeholder": "Theme name"}
# @aerofi.description Interactive theme previewer and switcher
# @aerofi.author Timur Iskakov
# @aerofi.authorURL https://github.com/frostymur
# @aerofi.show_search true
# @aerofi.columns 2

echo "Theme switcher..."
"#,
        )
        .unwrap();

        let target =
            Target::script_from_file(&file_path).expect("Failed to parse script with @aerofi tags");
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

        assert_eq!(name.as_ref(), "Theme Switcher");
        assert_eq!(mode, ScriptMode::Gui);
        assert_eq!(icon.as_deref(), Some("🎨"));

        assert_eq!(metadata.schema_version, Some(1));
        assert_eq!(metadata.title.as_deref(), Some("Theme Switcher"));
        assert_eq!(metadata.mode, Some(ScriptMode::Gui));
        assert_eq!(metadata.package_name.as_deref(), Some("aerofi Utilities"));
        assert_eq!(metadata.icon.as_deref(), Some("🎨"));
        assert_eq!(metadata.icon_dark.as_deref(), Some("🎭"));
        assert_eq!(metadata.refresh_time.as_deref(), Some("10m"));
        assert_eq!(metadata.needs_confirmation, Some(false));

        let arg1 = metadata.argument1.as_ref().expect("arg1 missing");
        assert_eq!(arg1.arg_type.as_deref(), Some("text"));
        assert_eq!(arg1.placeholder.as_deref(), Some("Theme name"));

        assert_eq!(
            metadata.description.as_deref(),
            Some("Interactive theme previewer and switcher")
        );
        assert_eq!(metadata.author.as_deref(), Some("Timur Iskakov"));
        assert_eq!(
            metadata.author_url.as_deref(),
            Some("https://github.com/frostymur")
        );

        assert_eq!(metatags.show_search, Some(true));
        assert_eq!(metatags.columns, Some(2));

        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn theme_switcher_example_script_parses_cleanly() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let script_path = manifest_dir.join("examples/scripts/theme_switcher.py");
        let target =
            Target::script_from_file(&script_path).expect("theme_switcher.py should parse");
        let Target::Script {
            name,
            mode,
            icon,
            metadata,
            ..
        } = target
        else {
            panic!("Expected Target::Script");
        };

        assert_eq!(name.as_ref(), "Theme Switcher");
        assert_eq!(mode, ScriptMode::Gui);
        assert_eq!(icon.as_deref(), Some("\u{f1fc}"));
        assert_eq!(metadata.package_name.as_deref(), Some("aerofi"));
    }

    #[test]
    fn clipboard_example_script_parses_cleanly() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let script_path = manifest_dir.join("examples/scripts/clipboard.py");
        let target = Target::script_from_file(&script_path).expect("clipboard.py should parse");
        let Target::Script {
            name,
            mode,
            icon,
            metadata,
            ..
        } = target
        else {
            panic!("Expected Target::Script");
        };

        assert_eq!(name.as_ref(), "Clipboard History");
        assert_eq!(mode, ScriptMode::Gui);
        assert_eq!(icon.as_deref(), Some("\u{f0ea}"));
        assert_eq!(metadata.package_name.as_deref(), Some("System"));
    }

    #[test]
    fn mode_example_scripts_parse_cleanly() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let scripts = [
            ("silent.sh", ScriptMode::Silent),
            ("compact.sh", ScriptMode::Compact),
            ("inline.sh", ScriptMode::Inline),
            ("full-output.sh", ScriptMode::FullOutput),
            ("pipe.sh", ScriptMode::Pipe),
            ("gui.sh", ScriptMode::Gui),
        ];

        for (filename, expected_mode) in scripts {
            let path = manifest_dir.join("examples/scripts").join(filename);
            let target = Target::script_from_file(&path)
                .unwrap_or_else(|| panic!("failed to parse {}", path.display()));
            let Target::Script { mode, .. } = target else {
                panic!("expected Target::Script for {}", filename);
            };
            assert_eq!(mode, expected_mode, "mode mismatch for {}", filename);
        }
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

    #[test]
    fn category_label_matches_target_variant() {
        let app = Target::App {
            name: "Safari".into(),
            path: Arc::from(PathBuf::from("/Applications/Safari.app")),
            icon_path: None,
        };
        let script = Target::Script {
            name: "hello".into(),
            path: Arc::from(PathBuf::from("/tmp/hello.sh")),
            mode: ScriptMode::FullOutput,
            icon: None,
            icon_image_path: None,
            metadata: Arc::new(RaycastMetadata::default()),
            metatags: ScriptMetatags::default(),
            inline_output: None,
        };
        let builtin = Target::reload_config();
        assert_eq!(app.category_label(), "Application");
        assert_eq!(script.category_label(), "Script");
        assert_eq!(builtin.category_label(), "Aerofi");
        assert_eq!(builtin.icon(), None);
    }
}
