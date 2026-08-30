//! The launcher UI: a keyboard-driven, fuzzy-filterable list of targets
//! (applications and scripts).
//!
//! There is no ready-made `InputText` in this GPUI revision, so the filter
//! query is owned here as a plain string and driven from the global
//! `observe_keystrokes` handler (see `main.rs`). That keeps us to an
//! append-only, backspace-only text model, which is all a launcher needs.

use gpui::{
    Context, CursorStyle, Render, ScrollStrategy, UniformListScrollHandle, Window, div, img,
    prelude::*, px, rgb, rgba, uniform_list,
};

use crate::core::config::AppConfig;
use crate::core::history::History;
use crate::core::item::{BuiltinAction, Target};
use crate::core::search::SearchIndex;
use crate::core::theme::{self, ThemeConfig, Widget};

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
    /// Launch history, appended to on every execution (frecency source).
    history: History,
    /// The app configuration this launcher was built from (re-read by
    /// "Reload Configuration").
    app_config: AppConfig,
    /// Active theme controlling every visual aspect of the launcher.
    theme: ThemeConfig,
}

impl Launcher {
    pub fn new(
        all: Vec<Target>,
        theme: ThemeConfig,
        app_config: AppConfig,
        history: History,
    ) -> Self {
        let max_results = app_config.general.max_results;
        let filtered = all.iter().take(max_results).cloned().collect();
        Self {
            all,
            filtered,
            query: String::new(),
            selected: 0,
            list: UniformListScrollHandle::new(),
            search: SearchIndex::new(&app_config.aliases, max_results),
            history,
            app_config,
            theme,
        }
    }

    /// Convenience: resolve a theme hex colour string to a `u32` for
    /// GPUI's `rgb()`, falling back to black on bad input.
    fn color(hex: &str) -> u32 {
        theme::parse_hex_color(hex).unwrap_or(0)
    }

    /// Handle a keystroke. Returns `true` when the window should be hidden
    /// afterwards (Esc, Enter after running, Cmd+E after opening the editor).
    pub fn handle_keystroke(&mut self, ks: &gpui::Keystroke) -> bool {
        // A configured key-combo shortcut (e.g. "cmd+r") runs its target
        // immediately; explicit config overrides the built-in bindings.
        if let Some((_, name)) = self
            .app_config
            .shortcuts
            .iter()
            .find(|(combo, _)| combo_matches(combo, ks))
            && let Some(item) = self.all.iter().find(|t| t.name() == name)
        {
            let item = item.clone();
            self.execute_item(&item);
            self.reset();
            return true;
        }
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
                    // A configured alias: typing it exactly runs its target
                    // immediately (no Enter needed).
                    if let Some(item) = self.alias_target() {
                        let item = item.clone();
                        self.execute_item(&item);
                        self.reset();
                        return true;
                    }
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
        self.filtered = self
            .search
            .filter_and_rank(&self.history, &self.all, &self.query);
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
        // A new query re-ranks the list, so start at the top.
        self.list.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn selected_item(&self) -> Option<&Target> {
        self.filtered.get(self.selected)
    }

    /// The target an alias points at, when the current query exactly
    /// matches the alias (typing it runs the target immediately).
    fn alias_target(&self) -> Option<&Target> {
        let name = self.app_config.aliases.get(&self.query)?;
        self.all.iter().find(|t| t.name() == name)
    }

    /// Run the highlighted target.
    fn execute_selected(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let item = item.clone();
        self.execute_item(&item);
    }

    /// Run a target: built-in actions act in place, apps open and scripts
    /// run via `sh`. On-disk launches are recorded in the history for
    /// frecency ranking.
    fn execute_item(&mut self, item: &Target) {
        match item {
            Target::Builtin {
                action: BuiltinAction::ReloadConfig,
                ..
            } => self.reload(),
            _ => {
                // Clone the identifier out before the mutable borrow for
                // `record_launch`.
                let identifier = item.identifier().into_owned();
                crate::core::executor::execute(item);
                self.history.record_launch(&identifier);
            }
        }
    }

    /// Re-read `config.toml`, rescan the targets and rebuild the search
    /// index (aliases, `max_results`, sources, ignored apps, script dirs).
    fn reload(&mut self) {
        let config = crate::core::config::AppConfig::load();
        let targets = crate::core::scanner::scan_all(&config);
        self.app_config = config;
        self.all = targets;
        self.search = SearchIndex::new(
            &self.app_config.aliases,
            self.app_config.general.max_results,
        );
        self.query.clear();
        self.refilter();
        self.selected = 0;
        println!("aerofi: configuration reloaded");
    }

    /// Open the highlighted script in `$EDITOR` (defaulting to `vim`).
    /// Applications have no source to edit, so this is a no-op for them.
    fn open_in_editor(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let path = match item {
            Target::Script { path, .. } => path.clone(),
            // Applications and built-in actions have no source to edit.
            Target::App { .. } | Target::Builtin { .. } => return,
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
        let t = &self.theme;

        // Build the main container: orientation from theme, padding and
        // corner radius from window config.
        let is_vertical = t.mainbox.orientation == "vertical";
        let mut container = div()
            .size_full()
            .bg(rgb(Self::color(&t.window.background)))
            .text_color(rgb(Self::color(&t.element.text_color)))
            .text_size(px(t.font.size))
            .flex()
            .p(px(t.window.padding))
            .rounded_lg()
            .gap(px(t.listview.spacing));

        if is_vertical {
            container = container.flex_col();
        } else {
            container = container.flex_row();
        }

        // Dynamically assemble children from theme.mainbox.children.
        for widget in &t.mainbox.children {
            match widget {
                Widget::InputBar => {
                    container = container.child(self.render_inputbar());
                }
                Widget::ListView => {
                    container = container.child(self.render_listview(cx));
                }
                Widget::Banner => {
                    if let Some(path) = t.banner.as_ref().and_then(|b| b.image_path.as_ref()) {
                        let height = t.banner.as_ref().map(|b| b.height).unwrap_or(120.0);
                        container = container.child(
                            img(path.as_str())
                                .w_full()
                                .h(px(height))
                                .object_fit(gpui::ObjectFit::Cover)
                                .rounded_md(),
                        );
                    }
                }
                _ => {}
            }
        }

        container
    }
}

impl Launcher {
    /// Render the input bar styled from `theme.inputbar`.
    fn render_inputbar(&self) -> gpui::AnyElement {
        let t = &self.theme;
        let ib = &t.inputbar;

        let placeholder_view = if self.query.is_empty() {
            div()
                .text_color(rgb(Self::color(&ib.placeholder_color)))
                .child(ib.placeholder.clone())
                .into_any()
        } else {
            div()
                .text_color(rgb(Self::color(&ib.text_color)))
                .child(self.query.clone())
                .into_any()
        };

        let icon_label = ib.icon.as_deref().unwrap_or("❯");
        let icon_color = ib.icon_color.as_deref().unwrap_or(&ib.text_color);

        let padding_h = ib.padding.first().copied().unwrap_or(12.0);
        let padding_v = ib.padding.get(1).copied().unwrap_or(16.0);
        let margin_bottom = ib.margin.get(2).copied().unwrap_or(8.0);

        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(px(ib.height))
            .px(px(padding_h))
            .py(px(padding_v))
            .mb(px(margin_bottom))
            .bg(rgb(Self::color(&ib.background)))
            .rounded(px(ib.corner_radius))
            .child(
                div()
                    .text_color(rgb(Self::color(icon_color)))
                    .child(icon_label.to_string()),
            )
            .child(placeholder_view)
            .into_any()
    }

    /// Render the result list styled from `theme.listview` and `theme.element`.
    fn render_listview(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;

        if self.filtered.is_empty() {
            return div()
                .flex_1()
                .px_2()
                .py_1()
                .text_color(rgb(Self::color(&t.listview.empty_text_color)))
                .child(t.listview.empty_text.clone())
                .into_any();
        }

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
    }

    /// Build a single list row for `filtered_ix` (position within `filtered`).
    fn render_row(&self, filtered_ix: usize) -> gpui::AnyElement {
        let item = &self.filtered[filtered_ix];
        let is_selected = filtered_ix == self.selected;
        let t = &self.theme;
        let el = &t.element;

        let (row_bg, name_color) = if is_selected {
            (
                rgb(Self::color(&el.selected.background)),
                rgb(Self::color(&el.selected.text_color)),
            )
        } else {
            (rgba(0x00000000), rgb(Self::color(&el.text_color)))
        };

        let icon_size = px(el.icon_size);
        let icon_element = if let Some(path) = item.icon_path() {
            img(path).w(icon_size).h(icon_size).rounded_sm().into_any()
        } else {
            let fallback = item.icon().unwrap_or("•");
            div()
                .w(icon_size)
                .text_color(rgb(Self::color(
                    t.inputbar
                        .icon_color
                        .as_deref()
                        .unwrap_or(&t.inputbar.text_color),
                )))
                .child(fallback.to_string())
                .into_any()
        };

        let pad_h = el.padding.first().copied().unwrap_or(8.0);
        let pad_v = el.padding.get(1).copied().unwrap_or(12.0);

        let row = div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .px(px(pad_h))
            .py(px(pad_v))
            .rounded(px(el.corner_radius))
            .cursor(CursorStyle::PointingHand)
            .bg(row_bg)
            .child(icon_element)
            .child(
                div()
                    .flex_1()
                    .text_color(name_color)
                    .child(item.name().to_string()),
            );

        // Right-aligned badge with the target's bound shortcuts, if any.
        let row = match self.shortcut_label(item.name()) {
            Some(label) => row.child(
                div()
                    .text_size(px(t.font.size - 2.0))
                    .text_color(rgb(Self::color(
                        el.description_color.as_deref().unwrap_or(&el.text_color),
                    )))
                    .child(label),
            ),
            None => row,
        };
        row.into_any()
    }

    /// The shortcuts bound to a target, for display next to its row: the
    /// global combo first, then the launcher-local one, in macOS glyph form
    /// (e.g. `"⌥G  ⌘R"`). `None` when the target has no bound shortcut.
    fn shortcut_label(&self, name: &str) -> Option<String> {
        let global = self
            .app_config
            .global_shortcuts
            .iter()
            .find(|(_, target)| target.as_str() == name)
            .map(|(combo, _)| combo.clone());
        let local = self
            .app_config
            .shortcuts
            .iter()
            .find(|(_, target)| target.as_str() == name)
            .map(|(combo, _)| combo.clone());
        let labels = [global, local]
            .into_iter()
            .flatten()
            .map(|combo| format_combo(&combo))
            .collect::<Vec<_>>();
        (!labels.is_empty()).then(|| labels.join("  "))
    }
}

/// Render a config combo (`"cmd+shift+r"`, `"opt+space"`) in macOS glyph
/// form: modifiers in the canonical order ⌃⌥⇧⌘, then the key glyph
/// (`"⌃⇧⌘R"`, `"⌥␣"`).
fn format_combo(combo: &str) -> String {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut cmd = false;
    let mut key = String::new();
    for token in combo.split('+') {
        let token = token.trim().to_ascii_lowercase();
        match token.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "cmd" | "command" | "super" => cmd = true,
            other if !other.is_empty() => key = other.to_string(),
            _ => {}
        }
    }
    let mut label = String::new();
    if ctrl {
        label.push('⌃');
    }
    if alt {
        label.push('⌥');
    }
    if shift {
        label.push('⇧');
    }
    if cmd {
        label.push('⌘');
    }
    label.push_str(&key_glyph(&key));
    label
}

/// macOS glyph for a key name (`"space"` -> `"␣"`, `"f12"` -> `"F12"`);
/// letters and digits are uppercased as-is.
fn key_glyph(key: &str) -> String {
    if let Some(digits) = key.strip_prefix('f')
        && let Ok(n) = digits.parse::<u8>()
        && (1..=12).contains(&n)
    {
        return format!("F{n}");
    }
    match key {
        "space" => "␣",
        "return" | "enter" => "⏎",
        "escape" | "esc" => "⎋",
        "tab" => "⇥",
        "backspace" => "⌫",
        "left" => "←",
        "right" => "→",
        "up" => "↑",
        "down" => "↓",
        _ => return key.to_uppercase(),
    }
    .to_string()
}

/// True when `combo` (e.g. "cmd+r" or "ctrl+shift+x") matches the pressed
/// keystroke: the same key and exactly the listed modifiers. Modifier
/// names: `cmd`/`command`/`super`, `ctrl`/`control`, `alt`/`option`/`opt`,
/// `shift`; the key is the remaining token (case-insensitive).
fn combo_matches(combo: &str, ks: &gpui::Keystroke) -> bool {
    let mut want = gpui::Modifiers::default();
    let mut key: Option<String> = None;
    for token in combo.split('+') {
        let token = token.trim().to_ascii_lowercase();
        match token.as_str() {
            "cmd" | "command" | "super" => want.platform = true,
            "ctrl" | "control" => want.control = true,
            "alt" | "option" | "opt" => want.alt = true,
            "shift" => want.shift = true,
            other if !other.is_empty() => key = Some(other.to_string()),
            _ => {}
        }
    }
    matches!(key.as_deref(), Some(k) if k == ks.key.to_ascii_lowercase()) && want == ks.modifiers
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

    fn cap_config(max_results: usize) -> AppConfig {
        let mut config = AppConfig::default();
        config.general.max_results = max_results;
        config
    }

    #[test]
    fn alias_resolves_target_by_exact_query() {
        let mut app_config = AppConfig::default();
        app_config
            .aliases
            .insert("rc".to_string(), "Reload Configuration".to_string());
        let mut l = Launcher::new(
            vec![item("Grep"), Target::reload_config()],
            ThemeConfig::default(),
            app_config,
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.query = "rc".to_string();
        assert_eq!(
            l.alias_target().map(|t| t.name()),
            Some("Reload Configuration")
        );
        l.query = "r".to_string();
        assert_eq!(l.alias_target(), None);
    }

    fn keystroke(key: &str, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: (key.len() == 1).then(|| key.to_string()),
        }
    }

    #[test]
    fn combo_matches_key_combinations() {
        let cmd_r = keystroke(
            "r",
            Modifiers {
                platform: true,
                ..Default::default()
            },
        );
        assert!(combo_matches("cmd+r", &cmd_r));
        assert!(combo_matches("command+r", &cmd_r));
        assert!(!combo_matches("ctrl+r", &cmd_r));
        assert!(!combo_matches("cmd+x", &cmd_r));
        assert!(!combo_matches("cmd+shift+r", &cmd_r));

        let plain_r = keystroke("r", Modifiers::default());
        assert!(combo_matches("r", &plain_r));
        assert!(!combo_matches("cmd+r", &plain_r));

        let ctrl_shift_x = keystroke(
            "x",
            Modifiers {
                control: true,
                shift: true,
                ..Default::default()
            },
        );
        assert!(combo_matches("ctrl+shift+x", &ctrl_shift_x));
        assert!(combo_matches("shift+ctrl+x", &ctrl_shift_x));
    }

    #[test]
    fn empty_query_shows_all_in_order() {
        let l = Launcher::new(
            vec![item("Git Status"), item("Clipboard History")],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
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
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
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
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
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
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
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
            ThemeConfig::default(),
            cap_config(2),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        assert_eq!(names(&l), vec!["A One".to_string(), "A Two".to_string()]);
        l.handle_keystroke(&key("a"));
        assert!(names(&l).len() <= 2);
    }

    #[test]
    fn escape_signals_hide_and_resets_query() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep")],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.handle_keystroke(&key("g"));
        assert_eq!(l.query, "g");
        assert!(l.handle_keystroke(&key("escape")));
        assert_eq!(l.query, "");
        assert_eq!(l.selected, 0);
    }

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
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.handle_keystroke(&key("down"));
        assert_eq!(deferred(&l), Some((1, ScrollStrategy::Nearest)));
    }

    #[test]
    fn format_combo_uses_macos_glyphs_in_canonical_order() {
        assert_eq!(format_combo("cmd+r"), "⌘R");
        assert_eq!(format_combo("opt+d"), "⌥D");
        assert_eq!(format_combo("ctrl+alt+f12"), "⌃⌥F12");
        assert_eq!(format_combo("cmd+shift+space"), "⇧⌘␣");
        assert_eq!(format_combo("alt+return"), "⌥⏎");
        assert_eq!(format_combo("cmd+f1"), "⌘F1");
        assert_eq!(format_combo("ctrl+left"), "⌃←");
    }

    #[test]
    fn shortcut_label_shows_global_then_local_combos() {
        let mut app_config = AppConfig::default();
        app_config
            .global_shortcuts
            .insert("opt+g".to_string(), "Marker".to_string());
        app_config
            .shortcuts
            .insert("cmd+r".to_string(), "Reload Configuration".to_string());
        app_config
            .global_shortcuts
            .insert("opt+m".to_string(), "Reload Configuration".to_string());
        let l = Launcher::new(
            vec![item("Marker"), item("Grep")],
            ThemeConfig::default(),
            app_config,
            History::test_new(PathBuf::new(), Vec::new()),
        );
        assert_eq!(l.shortcut_label("Marker").as_deref(), Some("⌥G"));
        assert_eq!(l.shortcut_label("Grep").as_deref(), None);
        assert_eq!(
            l.shortcut_label("Reload Configuration").as_deref(),
            Some("⌥M  ⌘R")
        );
    }

    #[test]
    fn typing_defers_scroll_back_to_top() {
        let mut l = Launcher::new(
            vec![item("Git Status"), item("Grep")],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.handle_keystroke(&key("g"));
        assert_eq!(deferred(&l), Some((0, ScrollStrategy::Top)));
    }
}
