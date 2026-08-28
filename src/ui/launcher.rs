//! The launcher UI: a keyboard-driven, fuzzy-filterable list of scripts.
//!
//! There is no ready-made `InputText` in this GPUI revision, so the filter
//! query is owned here as a plain string and driven from the global
//! `observe_keystrokes` handler (see `main.rs`). That keeps us to an
//! append-only, backspace-only text model, which is all a launcher needs.

use gpui::{Context, CursorStyle, Render, Window, div, prelude::*, px, rgb, rgba};

use crate::common::config::ThemeColors;
use crate::common::script_item::ScriptItem;
use crate::core::search::SearchIndex;

/// Root view: renders the filter field and the ranked list of scripts.
pub struct Launcher {
    /// Every parsed script, kept in name-sorted order (the "unfiltered" order).
    all: Vec<ScriptItem>,
    /// Indices into `all`, ranked by match score (best first).
    filtered: Vec<usize>,
    /// Current filter query.
    query: String,
    /// Position of the highlighted row within `filtered`.
    selected: usize,
    /// Reused nucleo matcher (it allocates a working set up front).
    search: SearchIndex,
    /// Palette for the dark launcher surface.
    theme: ThemeColors,
}

impl Launcher {
    pub fn new(all: Vec<ScriptItem>, theme: ThemeColors) -> Self {
        let filtered = (0..all.len()).collect();
        Self {
            all,
            filtered,
            query: String::new(),
            selected: 0,
            search: SearchIndex::new(),
            theme,
        }
    }

    /// Handle a keystroke. Returns `true` when the window should be hidden
    /// afterwards (Esc, Enter after running, Cmd+E after opening the editor).
    pub fn handle_keystroke(&mut self, ks: &gpui::Keystroke) -> bool {
        let cmd = ks.modifiers.platform;
        match (ks.key.as_str(), cmd) {
            ("escape", _) => {
                self.reset();
                true
            }
            ("up", false) => {
                self.move_selection(-1);
                false
            }
            ("down", false) => {
                self.move_selection(1);
                false
            }
            ("enter" | "return", false) => {
                self.execute_selected();
                self.reset();
                true
            }
            ("e", true) => {
                self.open_in_editor();
                self.reset();
                true
            }
            ("backspace", false) => {
                self.backspace();
                false
            }
            _ => {
                // Treat plain printable characters (no cmd/ctrl/alt) as filter input.
                if !cmd
                    && !ks.modifiers.control
                    && !ks.modifiers.alt
                    && ks.key != "tab"
                    && let Some(c) = ks.key_char.as_deref()
                    && !c.is_empty()
                    && !c.chars().any(char::is_control)
                {
                    self.query.push_str(c);
                    self.refilter();
                    self.selected = 0;
                }
                false
            }
        }
    }

    /// Clear the query and reselect the top (first) entry.
    fn reset(&mut self) {
        self.query.clear();
        self.refilter();
        self.selected = 0;
    }

    fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.refilter();
            self.selected = 0;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let len = self.filtered.len() as isize;
        self.selected = (self.selected as isize + delta).clamp(0, len - 1) as usize;
    }

    /// Re-run the fuzzy match for the current query and rebuild `filtered`.
    fn refilter(&mut self) {
        let names: Vec<&str> = self.all.iter().map(|i| i.name.as_str()).collect();
        self.filtered = self.search.search(&names, &self.query);
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    fn selected_item(&self) -> Option<&ScriptItem> {
        self.filtered.get(self.selected).map(|&i| &self.all[i])
    }

    /// Run the highlighted script. Its stdout+stderr inherit the parent's, so
    /// output lands in the terminal (stderr) instead of the GUI.
    fn execute_selected(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let path = item.path.clone();
        let name = item.name.clone();
        match std::process::Command::new(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(_) => {}
            Err(e) => eprintln!("aerofi: failed to run {name}: {e}"),
        }
    }

    /// Open the highlighted script in `$EDITOR` (defaulting to `vim`).
    fn open_in_editor(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let path = item.path.clone();
        let name = item.name.clone();
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
        // `EDITOR` may be "cmd -arg ..."; split into program + initial args.
        let mut parts = editor.split_whitespace();
        let program = parts.next().unwrap_or("vim");
        match std::process::Command::new(program)
            .args(parts)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(_) => {}
            Err(e) => eprintln!("aerofi: failed to open {name} in {editor}: {e}"),
        }
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let query_view = if self.query.is_empty() {
            div()
                .text_color(rgb(self.theme.dim))
                .child("Type to filter…")
                .into_any()
        } else {
            div()
                .text_color(rgb(self.theme.text))
                .child(self.query.clone())
                .into_any()
        };

        let rows: Vec<gpui::AnyElement> = if self.filtered.is_empty() {
            vec![
                div()
                    .px_2()
                    .py_1()
                    .text_color(rgb(self.theme.dim))
                    .child(format!("No matches for “{}”", self.query))
                    .into_any(),
            ]
        } else {
            self.filtered
                .iter()
                .enumerate()
                .map(|(pos, &i)| self.render_row(i, pos == self.selected))
                .collect()
        };

        div()
            .size_full()
            .bg(rgb(self.theme.bg))
            .text_color(rgb(self.theme.text))
            .text_size(px(15.))
            .flex()
            .flex_col()
            .p_3()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(self.theme.input_bg))
                    .rounded_md()
                    .px_3()
                    .py_2()
                    .child(div().text_color(rgb(self.theme.accent)).child("❯"))
                    .child(query_view),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .overflow_hidden()
                    .children(rows),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(self.theme.dim))
                    .child("↑↓ select · ⏎ run · ⌘E edit · esc close"),
            )
    }
}

impl Launcher {
    /// Build a single list row for the script at `all_idx`.
    fn render_row(&self, all_idx: usize, is_selected: bool) -> gpui::AnyElement {
        let item = &self.all[all_idx];
        let icon = item.icon.as_deref().unwrap_or("•");
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor(CursorStyle::PointingHand)
            .bg(if is_selected {
                rgb(self.theme.selection)
            } else {
                rgba(0x00000000)
            })
            .child(
                div()
                    .w(px(20.))
                    .text_color(rgb(self.theme.accent))
                    .child(icon.to_string()),
            )
            .child(
                div()
                    .text_color(if is_selected {
                        rgb(self.theme.text)
                    } else {
                        rgb(self.theme.text_muted)
                    })
                    .child(item.name.clone()),
            )
            .into_any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};
    use std::path::PathBuf;

    fn item(name: &str) -> ScriptItem {
        ScriptItem {
            name: name.to_string(),
            mode: crate::common::script_item::ScriptMode::FullOutput,
            icon: None,
            path: PathBuf::from(name),
        }
    }

    /// Build a keystroke the way the platform does for a plain key press.
    fn key(k: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: k.to_string(),
            key_char: (k.len() == 1).then(|| k.to_string()),
        }
    }

    fn names(l: &Launcher) -> Vec<String> {
        l.filtered.iter().map(|&i| l.all[i].name.clone()).collect()
    }

    #[test]
    fn empty_query_shows_all_in_order() {
        let l = Launcher::new(
            vec![item("Git Status"), item("Clipboard History")],
            ThemeColors::default(),
        );
        assert_eq!(
            names(&l),
            vec!["Git Status".to_string(), "Clipboard History".to_string()]
        );
    }

    #[test]
    fn typing_filters_fuzzy_and_excludes_non_matches() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Clipboard History"), item("Grep")],
            ThemeColors::default(),
        );
        l.handle_keystroke(&key("g"));
        let n = names(&l);
        assert!(n.contains(&"Git Status".to_string()));
        assert!(n.contains(&"Grep".to_string()));
        assert!(!n.contains(&"Clipboard History".to_string()));
    }

    #[test]
    fn backspace_restores_previous_results() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep")],
            ThemeColors::default(),
        );
        l.handle_keystroke(&key("g"));
        assert_eq!(names(&l).len(), 2);
        l.handle_keystroke(&key("g")); // "gg" matches neither
        assert!(names(&l).is_empty());
        l.handle_keystroke(&key("backspace")); // back to "g"
        assert!(names(&l).contains(&"Git Status".to_string()));
    }

    #[test]
    fn arrows_move_and_clamp_selection() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep"), item("Copy")],
            ThemeColors::default(),
        );
        assert_eq!(l.selected, 0);
        l.handle_keystroke(&key("down"));
        assert_eq!(l.selected, 1);
        l.handle_keystroke(&key("down"));
        assert_eq!(l.selected, 2);
        l.handle_keystroke(&key("down")); // clamps at the last row
        assert_eq!(l.selected, 2);
        l.handle_keystroke(&key("up"));
        assert_eq!(l.selected, 1);
    }

    #[test]
    fn escape_signals_hide_and_resets_query() {
        // Enter / Cmd+E spawn real processes, so they're exercised manually,
        // not here. Esc is pure: it resets the query and asks to hide.
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep")],
            ThemeColors::default(),
        );
        l.handle_keystroke(&key("g"));
        assert_eq!(l.query, "g");
        assert!(l.handle_keystroke(&key("escape")));
        assert_eq!(l.query, "");
        assert_eq!(l.selected, 0);
    }
}
