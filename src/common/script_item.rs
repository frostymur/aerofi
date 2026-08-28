//! The script data model shared by the indexer, the search and the UI.
//!
//! Parsing logic lives in [`crate::core::item`].

use std::path::PathBuf;

/// Output mode of a script (from `@raycast.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptMode {
    Silent,
    Compact,
    #[default]
    FullOutput,
}

impl ScriptMode {
    /// Canonical string form, as it appears in the annotation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Compact => "compact",
            Self::FullOutput => "fullOutput",
        }
    }

    /// Parse a mode value. Unknown or missing values default to `FullOutput`.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "silent" => Self::Silent,
            "compact" => Self::Compact,
            "fulloutput" => Self::FullOutput,
            _ => Self::FullOutput,
        }
    }
}

/// A single executable script plus its parsed metadata.
#[derive(Debug, Clone)]
pub struct ScriptItem {
    /// Display name (from `@raycast.title`, falling back to the file stem).
    pub name: String,
    /// Output mode (from `@raycast.mode`, defaulting to `FullOutput`).
    pub mode: ScriptMode,
    /// Icon (emoji or identifier) from `@raycast.icon`, if present.
    pub icon: Option<String>,
    /// Path to the script on disk.
    pub path: PathBuf,
}
