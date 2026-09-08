//! Data types for the launcher's state machine.

use crate::core::item::Target;

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
}
