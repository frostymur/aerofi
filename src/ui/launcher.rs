//! The launcher UI: a keyboard-driven, fuzzy-filterable list of targets
//! (applications and scripts).
//!
//! There is no ready-made `InputText` in this GPUI revision, so the filter
//! query is owned here as a plain string and driven from the global
//! `observe_keystrokes` handler (see `main.rs`). That keeps us to an
//! append-only, backspace-only text model, which is all a launcher needs.

use std::collections::HashMap;

use gpui::{
    Context, CursorStyle, Render, ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*,
    px, rgb, rgba, uniform_list,
};

use crate::common::config::ThemeColors;
use crate::core::item::Target;
use crate::core::search::SearchIndex;

/// Root view: renders the filter field and the ranked list of targets.
pub struct Launcher {
    /// Every indexed target, kept in name-sorted order (the "unfiltered" order).
    all: Vec<Target>,
    /// The ranked results for the current query (best first), capped at
    /// `max_results`.
    filtered: Vec<Target>,
    /// Current filter query.
    query: String,
    /// Position of the highlighted row within `filtered`.
    selected: usize,
    /// Scroll state of the targets list (wheel scrolling + arrow auto-scroll).
    list: UniformListScrollHandle,
    /// Reused nucleo matcher (it allocates a working set up front).
    search: SearchIndex,
    /// Palette for the dark launcher surface.
    theme: ThemeColors,
}

impl Launcher {
    pub fn new(
        all: Vec<Target>,
        theme: ThemeColors,
        aliases: HashMap<String, String>,
        max_results: usize,
    ) -> Self {
        let filtered = all.iter().take(max_results).cloned().collect();
        Self {
            all,
            filtered,
            query: String::new(),
            selected: 0,
            list: UniformListScrollHandle::new(),
            search: SearchIndex::new(&aliases, max_results),
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
        // Non-strict: scrolls only if the selected row is out of view.
        self.list
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
    }

    /// Re-run the fuzzy match for the current query and rebuild `filtered`.
    fn refilter(&mut self) {
        self.filtered = self.search.search(&self.all, &self.query);
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
        // A new query re-ranks the list, so start at the top.
        self.list.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn selected_item(&self) -> Option<&Target> {
        self.filtered.get(self.selected)
    }

    /// Run the highlighted target (apps open, scripts run via `sh`).
    fn execute_selected(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        crate::core::executor::execute(item);
    }

    /// Open the highlighted script in `$EDITOR` (defaulting to `vim`).
    /// Applications have no source to edit, so this is a no-op for them.
    fn open_in_editor(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let path = match item {
            Target::Script { path, .. } => path.clone(),
            Target::App { .. } => return,
        };
        let name = item.name().to_string();
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

        // Lazy-rendered, scrollable list of targets. Wheel scrolling is
        // handled by the list element (macOS-native direction); the scroll
        // handle is what `move_selection`/`refilter` use to follow the
        // selection.
        let list_view = if self.filtered.is_empty() {
            div()
                .flex_1()
                .px_2()
                .py_1()
                .text_color(rgb(self.theme.dim))
                .child(format!("No matches for “{}”", self.query))
                .into_any()
        } else {
            uniform_list(
                "targets",
                self.filtered.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                    range.map(|ix| this.render_row(ix)).collect()
                }),
            )
            .track_scroll(&self.list)
            .flex_1()
            .w_full()
            .into_any()
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
            .child(list_view)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(self.theme.dim))
                    .child("↑↓ scroll · ⏎ run · ⌘E edit · esc close"),
            )
    }
}

impl Launcher {
    /// Build a single list row for `filtered_ix` (position within `filtered`).
    fn render_row(&self, filtered_ix: usize) -> gpui::AnyElement {
        let item = &self.filtered[filtered_ix];
        let is_selected = filtered_ix == self.selected;
        let icon = item.icon().unwrap_or("•");
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
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
                    .child(item.name().to_string()),
            )
            .into_any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};
    use std::path::PathBuf;

    fn item(name: &str) -> Target {
        Target::Script {
            name: name.to_string(),
            mode: crate::core::item::ScriptMode::FullOutput,
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
        l.filtered.iter().map(|t| t.name().to_string()).collect()
    }

    #[test]
    fn empty_query_shows_all_in_order() {
        let l = Launcher::new(
            vec![item("Git Status"), item("Clipboard History")],
            ThemeColors::default(),
            HashMap::new(),
            20,
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
            HashMap::new(),
            20,
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
            HashMap::new(),
            20,
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
            HashMap::new(),
            20,
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
    fn list_is_capped_at_max_results() {
        let mut l = Launcher::new(
            vec![item("A One"), item("A Two"), item("A Three")],
            ThemeColors::default(),
            HashMap::new(),
            2,
        );
        assert_eq!(names(&l), vec!["A One".to_string(), "A Two".to_string()]);
        l.handle_keystroke(&key("a"));
        assert!(names(&l).len() <= 2);
    }

    #[test]
    fn escape_signals_hide_and_resets_query() {
        // Enter / Cmd+E spawn real processes, so they're exercised manually,
        // not here. Esc is pure: it resets the query and asks to hide.
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep")],
            ThemeColors::default(),
            HashMap::new(),
            20,
        );
        l.handle_keystroke(&key("g"));
        assert_eq!(l.query, "g");
        assert!(l.handle_keystroke(&key("escape")));
        assert_eq!(l.query, "");
        assert_eq!(l.selected, 0);
    }

    /// The actual scroll math (wheel direction, clamping, minimal
    /// auto-scroll) lives inside GPUI's `UniformList`; here we only verify
    /// that our input handlers hand the right deferred scrolls to the list.
    fn deferred(l: &Launcher) -> Option<(usize, ScrollStrategy)> {
        l.list
            .0
            .borrow()
            .deferred_scroll_to_item
            .map(|d| (d.item_index, d.strategy))
    }

    #[test]
    fn arrow_key_defers_scroll_to_selected_item() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep"), item("Copy")],
            ThemeColors::default(),
            HashMap::new(),
            20,
        );
        l.handle_keystroke(&key("down"));
        assert_eq!(deferred(&l), Some((1, ScrollStrategy::Nearest)));
    }

    #[test]
    fn typing_defers_scroll_back_to_top() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep")],
            ThemeColors::default(),
            HashMap::new(),
            20,
        );
        l.handle_keystroke(&key("g"));
        assert_eq!(deferred(&l), Some((0, ScrollStrategy::Top)));
    }
}
