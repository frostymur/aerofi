//! Script metadata extraction: reads a script file and pulls its
//! `@raycast.*` comment annotations into a [`ScriptItem`].
//!
//! Supported annotations (see `ARCHITECTURE.md`, "Script metadata"), read
//! from the first lines of each script:
//! - `# @raycast.title <name>` -> display name
//! - `# @raycast.mode <mode>`  -> `silent` | `compact` | `fullOutput`
//! - `# @raycast.icon <icon>`  -> emoji or icon identifier

use std::path::Path;

use crate::common::script_item::{ScriptItem, ScriptMode};

impl ScriptItem {
    /// Parse a single script file into a [`ScriptItem`].
    pub fn from_file(path: &Path) -> Option<Self> {
        let file_stem = path.file_stem()?.to_string_lossy().into_owned();
        let content = std::fs::read_to_string(path).ok()?;

        let mut name: Option<String> = None;
        let mut mode: Option<ScriptMode> = None;
        let mut icon: Option<String> = None;

        // Annotations live in the first few lines of the script.
        for line in content.lines().take(20) {
            // Keep only `# ...` comment lines.
            let Some(rest) = line.trim().strip_prefix('#') else {
                continue;
            };
            // Keep only `@raycast.` annotations.
            let Some(annotation) = rest.trim_start().strip_prefix("@raycast.") else {
                continue;
            };
            // Split into `field` and `value` at the first whitespace.
            let Some((field, value)) = annotation.split_once(|c: char| c.is_whitespace()) else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match field.trim() {
                "title" if name.is_none() => name = Some(value.to_string()),
                "mode" if mode.is_none() => mode = Some(ScriptMode::parse(value)),
                "icon" if icon.is_none() => icon = Some(value.to_string()),
                _ => {} // Unsupported fields (or duplicates) ignored in v0.1.
            }
        }

        Some(Self {
            name: name.unwrap_or(file_stem),
            mode: mode.unwrap_or_default(),
            icon,
            path: path.to_path_buf(),
        })
    }
}
