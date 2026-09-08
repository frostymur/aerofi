//! The launcher UI: a keyboard- and mouse-driven, fuzzy-filterable list of
//! targets (applications and scripts).
//!
//! There is no ready-made `InputText` in this GPUI revision, so the filter
//! query is owned here as a plain string and driven from the global
//! `observe_keystrokes` handler (see `main.rs`). That keeps us to an
//! append-only, backspace-only text model, which is all a launcher needs.
//!
//! Mouse: hovering a row moves the selection, clicking a row runs it, the
//! argument chips and confirmation buttons are clickable, and the list
//! scrolls with the wheel (handled natively by GPUI's list element).

mod helpers;
mod render;
mod state;
mod types;

pub use state::Launcher;
#[allow(unused_imports)]
pub use types::{LauncherAction, LauncherState};

// Private re-exports for test access via `use super::*`.
#[allow(unused_imports)]
use helpers::{combo_matches, format_combo};

#[cfg(test)]
mod tests;
