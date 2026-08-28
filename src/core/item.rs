//! The unified launcher element: an application or a shell script.
//!
//! [`Target`] is the single type the scanner produces, the search ranks and
//! the UI renders. Script metadata (`@raycast.*` comment tags) is extracted
//! in [`Target::script_from_file`].
//!
//! Supported annotations (see `ARCHITECTURE.md`, "Script metadata"), read
//! from the first lines of each script:
//! - `# @raycast.title <name>` -> display name
//! - `# @raycast.mode <mode>`  -> `compact` | `fullOutput`
//! - `# @raycast.icon <icon>`  -> emoji or icon identifier

use std::path::{Path, PathBuf};

/// Output mode of a script (from `@raycast.mode`).
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptMode {
    Compact,
    FullOutput,
}

impl ScriptMode {
    /// Canonical string form, as it appears in the annotation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::FullOutput => "fullOutput",
        }
    }

    /// Parse a mode value. Unknown or missing values default to `FullOutput`.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" => Self::Compact,
            _ => Self::FullOutput,
        }
    }
}

/// A single launchable element: an application bundle or a shell script.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// An application bundle (`.app`).
    App {
        /// Display name (the bundle directory name without the `.app` suffix).
        name: String,
        /// Path to the `.app` bundle on disk.
        path: PathBuf,
        /// Icon (emoji or identifier), if present.
        icon: Option<String>,
    },
    /// A shell script plus its parsed `@raycast.*` metadata.
    Script {
        /// Display name (from `@raycast.title`, falling back to the file stem).
        name: String,
        /// Path to the script on disk.
        path: PathBuf,
        /// Output mode (from `@raycast.mode`, defaulting to `FullOutput`).
        mode: ScriptMode,
        /// Icon (emoji or identifier) from `@raycast.icon`, if present.
        icon: Option<String>,
    },
}

impl Target {
    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            Self::App { name, .. } | Self::Script { name, .. } => name,
        }
    }

    /// Icon (emoji or identifier), if present.
    pub fn icon(&self) -> Option<&str> {
        match self {
            Self::App { icon, .. } | Self::Script { icon, .. } => icon.as_deref(),
        }
    }

    /// Parse a single script file into a `Target::Script`.
    pub fn script_from_file(path: &Path) -> Option<Self> {
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

        Some(Self::Script {
            name: name.unwrap_or(file_stem),
            mode: mode.unwrap_or(ScriptMode::FullOutput),
            icon,
            path: path.to_path_buf(),
        })
    }
}
