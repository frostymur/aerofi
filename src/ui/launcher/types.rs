//! Data types for the launcher's state machine.

use std::sync::Arc;

use crate::core::item::Target;
use crate::core::rofi_protocol::RofiRow;

/// Action to take after a keystroke is handled.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum LauncherAction {
    /// Do nothing special (just re-render).
    None,
    /// Hide the launcher window.
    Hide,
    /// Execute a script asynchronously (the caller handles the background work) with user-provided arguments.
    ExecuteScript(Target, Vec<String>),
    /// Copy output string to clipboard and hide.
    CopyToClipboardAndHide(String),
    /// Set the full output mode state (full page view in launcher).
    SetFullOutput { title: String, text: String },
    /// Update the inline_output of a script in `all[]` and re-render.
    SetInlineOutput {
        path: std::sync::Arc<std::path::Path>,
        output: Option<gpui::SharedString>,
    },
    /// Start a Rofi-mode interactive script session.
    StartRofiSession(Target, Vec<String>),
    /// Activate a dynamic plugin item.
    ActivatePlugin {
        plugin_name: String,
        plugin_id: String,
        action_code: u32,
    },
}

/// GPUI styling for one markdown text block, precomputed once when the
/// block is parsed (and again when the theme is reloaded): highlight ranges
/// with their theme-derived styles plus the byte ranges of inline code
/// spans. Lets the render pass clone two small vecs instead of re-walking
/// `MdText.marks` and re-deriving theme styles on every frame.
#[derive(Debug, Clone, PartialEq)]
pub struct MdStyled {
    pub highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>,
    pub code_ranges: Vec<std::ops::Range<usize>>,
}

/// State of the launcher UI.
#[derive(Debug, Clone, PartialEq)]
pub enum LauncherState {
    /// Normal search/list mode.
    Search,
    /// A fullOutput script is running; show a full page spinner.
    RunningFull { title: String },
    /// A fullOutput script finished; show a full page output view. The
    /// parsed markdown blocks live in `Launcher::full_output_blocks`.
    FullOutput { title: String },
    /// Prompting for arguments for a selected script.
    ArgumentInput {
        target: Target,
        args: Vec<crate::core::item::ScriptArgument>,
        values: Vec<String>,
        focused_index: usize,
    },
    /// Asking for confirmation before running a script with `needsConfirmation: true`.
    Confirming {
        target: Target,
        args_values: Vec<String>,
    },
    /// A Rofi-mode script is running interactively: the launcher shows the
    /// script's rows and relays selections back to the script's stdin.
    RofiMode {
        /// Title of the script (from `@raycast.title`).
        title: String,
        /// All rows in the current step. Shared via `Arc` so the per-frame
        /// list closures can clone it cheaply (atomic bump) instead of
        /// deep-cloning every row's string fields.
        rows: Arc<Vec<RofiRow>>,
        /// Indices into `rows` after fuzzy filtering.
        filtered_rows: Vec<usize>,
        /// Override for the input bar placeholder (from `\0prompt`).
        prompt: Option<String>,
        /// Status/info message (from `\0message`).
        message: Option<String>,
        /// If true, only listed items are selectable.
        no_custom: bool,
        /// Index into `filtered_rows` for the highlighted entry.
        selected: usize,
        /// Override grid columns (from `\0columns`).
        columns: Option<usize>,
        /// The current filter query typed by the user.
        query: String,
        /// Whether background loading is active (from `\0loading`).
        loading: bool,
        /// Whether live search is enabled (from `\0live-search`).
        live_search: bool,
        /// Specific active row indices (from `\0active`).
        active_indices: Vec<usize>,
        /// Arbitrary state string received via `\0data`.
        data: Option<String>,
        /// Parsed markdown blocks from `\0preview` or `\0preview-file`.
        preview_blocks: Option<Vec<crate::core::markdown::MdBlock>>,
        /// Precomputed markdown styling, parallel to `preview_blocks`.
        preview_styled: Option<Vec<Option<MdStyled>>>,
        /// Whether multi-selection is enabled.
        multi_select: bool,
        /// The set of row indices that have been toggled for multi-selection.
        toggled_indices: std::collections::HashSet<usize>,
        /// Whether inline Pango-like markup is enabled for row text.
        markup_rows: bool,
        /// Mode name from `@aerofi.preset` for theme `[presets.*]` overrides.
        layout: Option<String>,
    },
}
