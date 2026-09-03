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

use gpui::{
    Context, CursorStyle, Render, ScrollStrategy, UniformListScrollHandle, Window, div, img,
    prelude::*, px, rgb, rgba, size, uniform_list,
};

use crate::core::config::AppConfig;
use crate::core::history::History;
use crate::core::item::{BuiltinAction, ScriptMode, Target};
use crate::core::search::SearchIndex;
use crate::core::theme::{self, ThemeConfig, Widget, parse_hex_color};

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
    /// Current state of the launcher (e.g. normal search or showing script output).
    state: LauncherState,
    /// Scroll state for fullOutput mode.
    full_output_scroll: UniformListScrollHandle,
    /// Parsed markdown blocks of the current full-output result (empty
    /// while not showing one; cleared on hide to release the memory).
    full_output_blocks: Vec<crate::core::markdown::MdBlock>,
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
            state: LauncherState::Search,
            full_output_scroll: UniformListScrollHandle::new(),
            full_output_blocks: Vec::new(),
        }
    }

    /// Convenience: resolve a theme hex colour string to a `u32` for
    /// GPUI's `rgb()`, falling back to black on bad input.
    fn color(hex: &str) -> u32 {
        theme::parse_hex_color(hex).unwrap_or(0)
    }

    /// Handle a keystroke. Returns the action that the host (main.rs) should perform.
    pub fn handle_keystroke(&mut self, ks: &gpui::Keystroke) -> LauncherAction {
        // Full-output pages (spinner and result view) swallow every
        // keystroke; only Escape returns to the search list. Argument
        // prompts and confirmations fall through so their own handlers run.
        if matches!(
            &self.state,
            LauncherState::RunningFull { .. } | LauncherState::FullOutput { .. }
        ) {
            if ks.key == "escape" {
                self.back_from_full_output();
            }
            return LauncherAction::None;
        }

        // A configured key-combo shortcut (e.g. "cmd+r") runs its target
        // immediately; explicit config overrides the built-in bindings.
        // Only honoured in plain search mode — while an argument prompt or
        // confirmation is up, every keystroke belongs to that prompt.
        if matches!(self.state, LauncherState::Search)
            && let Some((_, name)) = self
                .app_config
                .shortcuts
                .iter()
                .find(|(combo, _)| combo_matches(combo, ks))
            && let Some(item) = self.all.iter().find(|t| t.name() == name)
        {
            let item = item.clone();
            let action = self.execute_item(&item);
            if matches!(
                action,
                LauncherAction::Hide | LauncherAction::ExecuteScript(..)
            ) {
                self.reset();
            }
            return action;
        }
        let cmd = ks.modifiers.platform;
        match (ks.key.as_str(), cmd) {
            ("escape", _) => {
                if !matches!(self.state, LauncherState::Search) {
                    self.state = LauncherState::Search;
                    return LauncherAction::None;
                }
                self.reset();
                LauncherAction::Hide
            }
            ("enter" | "return", false)
                if matches!(&self.state, LauncherState::Confirming { .. }) =>
            {
                self.confirm_and_run()
            }
            ("enter" | "return", false)
                if matches!(&self.state, LauncherState::ArgumentInput { .. }) =>
            {
                if let LauncherState::ArgumentInput {
                    target,
                    args,
                    values,
                    focused_index,
                } = &mut self.state
                {
                    if *focused_index < args.len() - 1 {
                        *focused_index += 1;
                        return LauncherAction::None;
                    }
                    let t = target.clone();
                    let vals = values.clone();
                    if t.needs_confirmation() {
                        self.state = LauncherState::Confirming {
                            target: t,
                            args_values: vals,
                        };
                        return LauncherAction::None;
                    } else {
                        self.state = LauncherState::Search;
                        let action = self.execute_target_with_args(&t, vals);
                        if matches!(
                            action,
                            LauncherAction::Hide
                                | LauncherAction::ExecuteScript(..)
                                | LauncherAction::SetFullOutput { .. }
                        ) {
                            self.reset();
                        }
                        return action;
                    }
                }
                LauncherAction::None
            }
            ("tab", false) if matches!(&self.state, LauncherState::ArgumentInput { .. }) => {
                if let LauncherState::ArgumentInput {
                    args,
                    focused_index,
                    ..
                } = &mut self.state
                {
                    *focused_index = (*focused_index + 1) % args.len();
                }
                LauncherAction::None
            }
            ("backspace", false) if matches!(&self.state, LauncherState::ArgumentInput { .. }) => {
                if let LauncherState::ArgumentInput {
                    values,
                    focused_index,
                    ..
                } = &mut self.state
                {
                    if values[*focused_index].pop().is_none() && *focused_index > 0 {
                        *focused_index -= 1;
                    }
                }
                LauncherAction::None
            }
            _ if matches!(&self.state, LauncherState::ArgumentInput { .. }) => {
                if !cmd
                    && !ks.modifiers.control
                    && !ks.modifiers.alt
                    && ks.key != "tab"
                    && let Some(c) = ks.key_char.as_deref()
                    && !c.is_empty()
                    && !c.chars().any(char::is_control)
                {
                    if let LauncherState::ArgumentInput {
                        values,
                        focused_index,
                        ..
                    } = &mut self.state
                    {
                        values[*focused_index].push_str(c);
                    }
                }
                LauncherAction::None
            }
            _ if !matches!(self.state, LauncherState::Search) => LauncherAction::None,
            ("up", false) => {
                let cols = self.theme.listview.columns;
                let step = if cols > 1 { cols as isize } else { 1 };
                self.move_selection(-step);
                LauncherAction::None
            }
            ("down", false) => {
                let cols = self.theme.listview.columns;
                let step = if cols > 1 { cols as isize } else { 1 };
                self.move_selection(step);
                LauncherAction::None
            }
            ("left", false) if self.theme.listview.columns > 1 => {
                self.move_selection(-1);
                LauncherAction::None
            }
            ("right", false) if self.theme.listview.columns > 1 => {
                self.move_selection(1);
                LauncherAction::None
            }
            ("enter" | "return", false) => {
                let action = self.execute_selected();
                if matches!(
                    action,
                    LauncherAction::Hide
                        | LauncherAction::ExecuteScript(..)
                        | LauncherAction::SetFullOutput { .. }
                ) {
                    self.reset();
                }
                action
            }
            ("e", true) => {
                self.open_in_editor();
                self.reset();
                LauncherAction::Hide
            }
            ("backspace", false) => {
                self.backspace();
                LauncherAction::None
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
                        let action = self.execute_item(&item);
                        if matches!(
                            action,
                            LauncherAction::Hide | LauncherAction::ExecuteScript(..)
                        ) {
                            self.reset();
                        }
                        return action;
                    }
                }
                LauncherAction::None
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
        let cols = self.theme.listview.columns;
        let scroll_ix = if cols > 1 {
            self.selected / cols
        } else {
            self.selected
        };
        // Non-strict: scrolls only if the selected row is out of view.
        self.list.scroll_to_item(scroll_ix, ScrollStrategy::Nearest);
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

    /// Called when the window is hidden.  Drops decoded GPU texture
    /// references held by `filtered` Targets so macOS can reclaim the
    /// memory while the launcher is not on screen.
    pub fn on_hide(&mut self) {
        self.filtered.clear();
        self.full_output_blocks.clear();
    }

    /// Called when the window is shown.  Refills `filtered` from `all`
    /// so the next render creates fresh `img()` elements that GPUI will
    /// decode on demand.
    pub fn on_show(&mut self) {
        self.refilter();
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
    fn execute_selected(&mut self) -> LauncherAction {
        let Some(item) = self.selected_item() else {
            return LauncherAction::None;
        };
        let item = item.clone();
        self.execute_item(&item)
    }

    /// Run a target: built-in actions act in place, apps open and scripts
    /// run asynchronously via `LauncherAction::ExecuteScript`.
    fn execute_item(&mut self, item: &Target) -> LauncherAction {
        match item {
            Target::Builtin {
                action: BuiltinAction::ReloadConfig,
                ..
            } => {
                self.reload();
                LauncherAction::Hide
            }
            Target::Script { .. } => {
                let args = item.arguments();
                if !args.is_empty() {
                    let arg_clones: Vec<_> = args.into_iter().cloned().collect();
                    let len = arg_clones.len();
                    self.state = LauncherState::ArgumentInput {
                        target: item.clone(),
                        args: arg_clones,
                        values: vec![String::new(); len],
                        focused_index: 0,
                    };
                    return LauncherAction::None;
                } else if item.needs_confirmation() {
                    self.state = LauncherState::Confirming {
                        target: item.clone(),
                        args_values: Vec::new(),
                    };
                    return LauncherAction::None;
                }

                self.execute_target_with_args(item, Vec::new())
            }
            Target::App { .. } => {
                let identifier = item.identifier();
                crate::core::executor::execute(item);
                self.history.record_launch(identifier);
                LauncherAction::Hide
            }
        }
    }

    fn execute_target_with_args(&mut self, item: &Target, args: Vec<String>) -> LauncherAction {
        match item {
            Target::Script { mode, name, .. } => {
                let identifier = item.identifier();
                self.history.record_launch(identifier);

                let mode = *mode;
                let title = name.to_string();

                match mode {
                    ScriptMode::Silent => LauncherAction::ExecuteScript(item.clone(), args),
                    ScriptMode::Pipe => LauncherAction::ExecuteScript(item.clone(), args),
                    ScriptMode::FullOutput => {
                        self.state = LauncherState::RunningFull { title };
                        LauncherAction::ExecuteScript(item.clone(), args)
                    }
                    ScriptMode::Compact => LauncherAction::ExecuteScript(item.clone(), args),
                    ScriptMode::Inline => LauncherAction::ExecuteScript(item.clone(), args),
                }
            }
            Target::Builtin { .. } | Target::App { .. } => LauncherAction::None,
        }
    }

    pub fn set_full_output(&mut self, title: String, text: String) {
        // Parse once here, not per render: the view re-renders on every
        // keystroke while visible, and output can be large.
        self.full_output_blocks = crate::core::markdown::parse(&text);
        self.state = LauncherState::FullOutput { title };
        // We can't scroll here easily because we don't have cx, but GPUI UniformListScrollHandle
        // might not need it until render.
    }

    /// Run the side effects of a `LauncherAction` produced inside the view.
    ///
    /// Both input paths funnel through here: the keystroke observer in
    /// `main.rs` and the mouse listeners on the launcher's elements. Async
    /// work is spawned, so this returns immediately.
    pub fn perform_action(&mut self, action: LauncherAction, cx: &mut Context<Self>) {
        match action {
            LauncherAction::None => {}
            LauncherAction::Hide => {
                self.on_hide();
                cx.notify();
                crate::ui::window::hide();
            }
            LauncherAction::CopyToClipboardAndHide(text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                self.on_hide();
                cx.notify();
                crate::ui::window::hide();
            }
            LauncherAction::SetFullOutput { title, text } => {
                self.set_full_output(title, text);
                cx.notify();
            }
            LauncherAction::SetInlineOutput { path, output } => {
                self.apply_inline_output(&path, output);
                cx.notify();
            }
            LauncherAction::ExecuteScript(target, args) => {
                // `silent` hides the launcher window immediately; the toast
                // takes over. Done here (not in the spawn) because we hold
                // the view and can't re-enter it from the async task.
                if let Target::Script { mode, .. } = &target
                    && *mode == ScriptMode::Silent
                {
                    self.on_hide();
                    crate::ui::window::hide_launcher_only();
                }
                let theme = self.theme.clone();
                let view = cx.entity();
                crate::ui::execute::execute_script(cx, view, theme, target, args);
                cx.notify();
            }
        }
    }

    /// Run the pending confirmed target (the "Yes" of the confirmation
    /// prompt). Shared by the Enter key and the Yes-button click.
    pub fn confirm_and_run(&mut self) -> LauncherAction {
        let LauncherState::Confirming {
            target,
            args_values,
        } = &self.state
        else {
            return LauncherAction::None;
        };
        let t = target.clone();
        let a = args_values.clone();
        self.state = LauncherState::Search;
        let action = self.execute_target_with_args(&t, a);
        if matches!(
            action,
            LauncherAction::Hide
                | LauncherAction::ExecuteScript(..)
                | LauncherAction::SetFullOutput { .. }
        ) {
            self.reset();
        }
        action
    }

    /// Focus the `index`-th argument of the active argument prompt
    /// (argument chip click).
    pub fn focus_argument(&mut self, index: usize) {
        if let LauncherState::ArgumentInput {
            args,
            focused_index,
            ..
        } = &mut self.state
            && index < args.len()
        {
            *focused_index = index;
        }
    }

    /// Leave the full-output page (spinner or result) and return to the
    /// search list. Shared by Escape and the "Back" header click.
    pub fn back_from_full_output(&mut self) {
        if matches!(
            self.state,
            LauncherState::RunningFull { .. } | LauncherState::FullOutput { .. }
        ) {
            self.state = LauncherState::Search;
            self.full_output_blocks.clear();
        }
    }

    /// Called by main.rs to update an inline script's cached subtitle.
    pub fn apply_inline_output(
        &mut self,
        path: &std::path::Path,
        output: Option<gpui::SharedString>,
    ) {
        for target in &mut self.all {
            let is_match = match target {
                Target::Script {
                    path: p,
                    mode: ScriptMode::Inline,
                    ..
                } => p.as_ref() == path,
                _ => false,
            };
            if is_match {
                target.set_inline_output(output);
                break;
            }
        }
        // Only rebuild search results if the window is visible.
        // If hidden, the window will re-filter on the next show anyway,
        // avoiding background memory leaks from GPUI image caching.
        if crate::ui::window::is_visible() {
            self.refilter();
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
            .arg(path.as_os_str())
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;

        // Apply runtime metatags from the currently selected script, if any.
        let selected_metatags = self
            .selected_item()
            .and_then(|item| item.metatags())
            .cloned()
            .unwrap_or_default();

        let show_search = selected_metatags.show_search.unwrap_or(true);
        let columns = selected_metatags.columns.unwrap_or(t.listview.columns);

        // Determine whether the list should be visible.
        let require_input = t.listview.require_input.unwrap_or(false);
        let should_show_list = if require_input {
            !self.query.trim().is_empty() && !self.filtered.is_empty()
        } else {
            true
        };

        let ib_height = t.inputbar.height;
        let pad_v = t.window.padding;
        let margin_bottom = t.inputbar.margin.get(2).copied().unwrap_or(8.0);

        let target_height = match &self.state {
            LauncherState::RunningFull { .. } | LauncherState::FullOutput { .. } => t.window.height,
            LauncherState::Search
            | LauncherState::ArgumentInput { .. }
            | LauncherState::Confirming { .. } => {
                if require_input {
                    if should_show_list {
                        let item_h = t.element.padding.get(1).copied().unwrap_or(12.0) * 2.0
                            + t.element.icon_size;
                        let list_h = (self.filtered.len() as f32) * (item_h + t.listview.spacing);
                        let total = ib_height + margin_bottom + list_h + pad_v * 2.0;
                        total.min(t.window.height)
                    } else {
                        ib_height + margin_bottom + pad_v * 2.0
                    }
                } else {
                    t.window.height
                }
            }
        };
        window.resize(size(px(t.window.width), px(target_height)));

        // Inner content: the actual launcher widgets or full page views.
        let inner = if let LauncherState::FullOutput { title } = &self.state {
            self.render_full_output(cx, title)
        } else if let LauncherState::RunningFull { title } = &self.state {
            self.render_full_output_running(title)
        } else {
            let is_vertical = t.mainbox.orientation == "vertical";
            let mut inner_box = div().flex_1().flex().gap(px(t.listview.spacing));

            if is_vertical {
                inner_box = inner_box.flex_col();
            } else {
                inner_box = inner_box.flex_row();
            }

            for widget in &t.mainbox.children {
                match widget {
                    Widget::InputBar if !show_search => {}
                    Widget::InputBar => {
                        inner_box = inner_box.child(self.render_inputbar(cx));
                    }
                    Widget::ListView => {
                        match &self.state {
                            LauncherState::Search => {
                                if should_show_list {
                                    inner_box = inner_box.child(self.render_listview(cx, columns));
                                }
                            }
                            LauncherState::Confirming { target, .. } => {
                                inner_box = inner_box.child(self.render_confirmation(target, cx));
                            }
                            LauncherState::ArgumentInput { .. } => {
                                // Argument options list (dropdown) could be rendered here.
                            }
                            _ => {}
                        }
                    }
                    Widget::Banner => {
                        if let Some(path) = t.banner.as_ref().and_then(|b| b.image_path.as_ref()) {
                            let resolved = expand_tilde_path(path);
                            let height = t.banner.as_ref().map(|b| b.height).unwrap_or(120.0);
                            inner_box = inner_box.child(
                                img(std::path::PathBuf::from(resolved))
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
            inner_box.into_any()
        };

        // Wrap with background colour, padding, and optional background image.
        let opacity = t.window.background_opacity.unwrap_or(1.0);
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(Self::color(&t.window.background)))
            .text_color(rgb(Self::color(&t.element.text_color)))
            .text_size(px(t.font.size))
            .rounded(px(t.window.corner_radius))
            .overflow_hidden();

        if t.window.border_width > 0.0 {
            root = root
                .border(px(t.window.border_width))
                .border_color(rgb(Self::color(&t.window.border_color)));
        }

        if opacity < 1.0 {
            // Apply alpha to the root background colour.
            let hex = parse_hex_color(&t.window.background).unwrap_or(0);
            let alpha = (opacity * 255.0) as u32;
            root = root.bg(rgba((hex << 8) | alpha));
        }

        let content = div()
            .flex_1()
            .flex()
            .flex_col()
            .p(px(t.window.padding))
            .child(inner);

        match t.window.background_image.as_deref() {
            Some(bg_path) => {
                let resolved = expand_tilde_path(bg_path);
                let position = t.window.background_position.as_deref().unwrap_or("cover");
                match position {
                    "left" => {
                        root = root.child(
                            div()
                                .flex()
                                .flex_row()
                                .size_full()
                                .child(
                                    img(std::path::PathBuf::from(&resolved))
                                        .h_full()
                                        .w(px(t.window.width * 0.4))
                                        .object_fit(gpui::ObjectFit::Cover),
                                )
                                .child(content.flex_1()),
                        );
                    }
                    "right" => {
                        root = root.child(
                            div()
                                .flex()
                                .flex_row()
                                .size_full()
                                .child(content.flex_1())
                                .child(
                                    img(std::path::PathBuf::from(&resolved))
                                        .h_full()
                                        .w(px(t.window.width * 0.4))
                                        .object_fit(gpui::ObjectFit::Cover),
                                ),
                        );
                    }
                    _ => {
                        // "cover" or unknown: full background
                        root = root.child(
                            img(std::path::PathBuf::from(&resolved))
                                .absolute()
                                .size_full()
                                .object_fit(gpui::ObjectFit::Cover),
                        );
                        root = root.child(content);
                    }
                }
            }
            None => {
                root = root.child(content);
            }
        }

        root
    }
}

impl Launcher {
    /// Render the input bar styled from `theme.inputbar`.
    fn render_inputbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let ib = &t.inputbar;

        let inner_view = if let LauncherState::ArgumentInput {
            target,
            args,
            values,
            focused_index,
        } = &self.state
        {
            let mut row = div().flex().flex_row().items_center().gap_2().child(
                div()
                    .text_color(rgb(Self::color(&t.element.text_color)))
                    .child(target.name().to_string()),
            );

            for (i, arg) in args.iter().enumerate() {
                let is_focused = i == *focused_index;
                let bg_color = if is_focused {
                    rgb(Self::color(&t.element.selected.background))
                } else {
                    rgba(0x00000000)
                };
                let border_color = if is_focused {
                    rgb(Self::color(&t.element.selected.background))
                } else {
                    rgb(Self::color(&ib.placeholder_color))
                };
                let text_val = &values[i];
                let display_text = if text_val.is_empty() {
                    arg.placeholder.as_deref().unwrap_or("...")
                } else {
                    text_val
                };
                let t_color = if text_val.is_empty() {
                    rgb(Self::color(&ib.placeholder_color))
                } else {
                    rgb(Self::color(&ib.text_color))
                };
                row = row.child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(bg_color)
                        .border_1()
                        .border_color(border_color)
                        .text_color(t_color)
                        .cursor(CursorStyle::PointingHand)
                        .id(format!("arg-chip-{i}"))
                        .on_click(cx.listener(move |this, event, _window, cx| {
                            if is_primary_click(event) {
                                this.focus_argument(i);
                                cx.notify();
                            }
                        }))
                        .child(display_text.to_string()),
                );
            }
            row.into_any()
        } else {
            if self.query.is_empty() {
                div()
                    .flex_1()
                    .text_color(rgb(Self::color(&ib.placeholder_color)))
                    .child(ib.placeholder.clone())
                    .into_any()
            } else {
                div()
                    .flex_1()
                    .text_color(rgb(Self::color(&ib.text_color)))
                    .child(self.query.clone())
                    .into_any()
            }
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
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(ib.height - padding_v * 2.0))
                    .h(px(ib.height - padding_v * 2.0))
                    .text_size(px(ib.height * 0.4))
                    .text_color(rgb(Self::color(icon_color)))
                    .child(icon_label.to_string()),
            )
            .child(inner_view)
            .into_any()
    }

    fn render_confirmation(&self, target: &Target, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let text_color = rgb(Self::color(&t.element.text_color));
        let sel_bg = rgb(Self::color(&t.element.selected.background));

        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .text_xl()
                    .text_color(text_color)
                    .child(format!("Run '{}'?", target.name())),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(sel_bg)
                            .text_color(text_color)
                            .cursor(CursorStyle::PointingHand)
                            .id("confirm-yes")
                            .on_click(cx.listener(move |this, event, _window, cx| {
                                if is_primary_click(event) {
                                    let action = this.confirm_and_run();
                                    this.perform_action(action, cx);
                                }
                            }))
                            .child("Yes (Enter)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(sel_bg)
                            .text_color(text_color)
                            .cursor(CursorStyle::PointingHand)
                            .id("confirm-no")
                            .on_click(cx.listener(move |this, event, _window, cx| {
                                if is_primary_click(event) {
                                    this.state = LauncherState::Search;
                                    cx.notify();
                                }
                            }))
                            .child("No (Esc)"),
                    ),
            )
            .into_any()
    }

    fn render_full_output_running(&self, title: &str) -> gpui::AnyElement {
        let t = &self.theme;
        let sel_bg = rgb(Self::color(&t.element.selected.background));
        let sel_text = rgb(Self::color(&t.element.selected.text_color));

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded(px(6.0))
                    .bg(sel_bg)
                    .text_sm()
                    .text_color(sel_text)
                    .child("▶ Running"),
            )
            .child(
                div()
                    .text_base()
                    .text_color(rgb(Self::color(&t.element.text_color)))
                    .child(title.to_string()),
            )
            .into_any_element()
    }

    fn render_full_output(&self, cx: &mut Context<Self>, title: &str) -> gpui::AnyElement {
        let t = &self.theme;

        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .pb(px(12.0))
            .border_b_1()
            .border_color(rgb(Self::color(&t.window.border_color)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_color(rgb(0x7aa2f7)) // some accent color or back button style
                            .text_sm()
                            .cursor(CursorStyle::PointingHand)
                            .id("full-output-back")
                            .on_click(cx.listener(move |this, event, _window, cx| {
                                if is_primary_click(event) {
                                    this.back_from_full_output();
                                    cx.notify();
                                }
                            }))
                            .child("❮ Back (Esc)"),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(rgb(Self::color(&t.element.text_color)))
                            .child(title.to_string()),
                    ),
            )
            .child(div().text_xs().text_color(rgb(0x565f89)).child("↵ Rerun"));

        let block_count = self.full_output_blocks.len();
        let body = if block_count == 0 {
            div()
                .flex_1()
                .text_sm()
                .font_family("JetBrains Mono")
                .text_color(rgb(Self::color(&t.inputbar.placeholder_color)))
                .child("(no output)")
                .into_any()
        } else {
            gpui::uniform_list(
                "full_output_blocks",
                block_count,
                cx.processor(
                    move |this: &mut Launcher, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|i| this.render_md_block(&this.full_output_blocks[i]))
                            .collect()
                    },
                ),
            )
            .flex_1()
            .w_full()
            .track_scroll(&self.full_output_scroll)
            .into_any()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// Render one markdown block of the full-output view.
    fn render_md_block(&self, block: &crate::core::markdown::MdBlock) -> gpui::AnyElement {
        use crate::core::markdown::MdBlock;
        let t = &self.theme;
        let text_color = rgb(Self::color(&t.element.text_color));
        let dim_color = rgb(Self::color(&t.inputbar.placeholder_color));
        let mono = gpui::SharedString::from("JetBrains Mono");

        let base = gpui::TextStyle {
            font_size: px(t.font.size).into(),
            color: Self::hsla(&t.element.text_color),
            ..Default::default()
        };

        match block {
            MdBlock::Heading { level, text } => {
                let scale = match *level {
                    1 => 1.5,
                    2 => 1.3,
                    3 => 1.15,
                    _ => 1.0,
                };
                let mut style = base.clone();
                style.font_size = px(t.font.size * scale).into();
                style.font_weight = gpui::FontWeight::BOLD;
                apply_md_style(div().w_full().mt_2(), &style)
                    .child(self.styled_md_text(text))
                    .into_any()
            }
            MdBlock::Paragraph(text) => apply_md_style(div().w_full(), &base)
                .child(self.styled_md_text(text))
                .into_any(),
            MdBlock::Blockquote(text) => {
                let mut style = base.clone();
                style.color = Self::hsla(&t.inputbar.placeholder_color);
                apply_md_style(
                    div()
                        .w_full()
                        .border_l_2()
                        .border_color(rgb(Self::color(&t.window.border_color)))
                        .pl_3(),
                    &style,
                )
                .child(self.styled_md_text(text))
                .into_any()
            }
            MdBlock::CodeBlock { lang, text } => {
                let mut box_ = div()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(Self::color(&t.window.border_color)))
                    .bg(rgb(Self::color(&t.inputbar.background)))
                    .p_3();
                if let Some(lang) = lang {
                    box_ = box_.child(
                        div()
                            .text_xs()
                            .text_color(dim_color)
                            .mb_1()
                            .child(lang.clone()),
                    );
                }
                box_ = box_.child(
                    div()
                        .font_family(mono)
                        .text_sm()
                        .text_color(text_color)
                        .child(text.clone()),
                );
                box_.into_any()
            }
            MdBlock::ListItem { number, text } => {
                let marker = number.map_or_else(|| "•".to_string(), |n| format!("{n}."));
                apply_md_style(div().w_full().flex().flex_row().gap_2(), &base)
                    .child(div().text_color(text_color).child(marker))
                    .child(div().flex_1().child(self.styled_md_text(text)))
                    .into_any()
            }
            MdBlock::Rule => div()
                .w_full()
                .h(px(1.0))
                .my_1()
                .bg(rgb(Self::color(&t.window.border_color)))
                .into_any(),
            MdBlock::Plain(text) => div()
                .w_full()
                .font_family(mono)
                .text_sm()
                .text_color(dim_color)
                .child(text.clone())
                .into_any(),
        }
    }

    /// Build a GPUI `StyledText` element for markdown text: one text layout
    /// per block with per-range highlights for inline emphasis and a
    /// monospace font override for inline code. The base style (family,
    /// size, colour) is inherited from the parent element's `text_style`.
    fn styled_md_text(&self, md: &crate::core::markdown::MdText) -> gpui::StyledText {
        use crate::core::markdown::InlineKind;

        let mut highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
        let mut code_ranges: Vec<(std::ops::Range<usize>, gpui::SharedString)> = Vec::new();
        for mark in &md.marks {
            let style = match mark.kind {
                InlineKind::Bold => gpui::HighlightStyle {
                    font_weight: Some(gpui::FontWeight::BOLD),
                    ..Default::default()
                },
                InlineKind::Italic => gpui::HighlightStyle {
                    font_style: Some(gpui::FontStyle::Italic),
                    ..Default::default()
                },
                InlineKind::Strikethrough => gpui::HighlightStyle {
                    strikethrough: Some(gpui::StrikethroughStyle::default()),
                    ..Default::default()
                },
                InlineKind::Link => gpui::HighlightStyle {
                    color: Some(Self::hsla_hex(0x7aa2f7)),
                    underline: Some(gpui::UnderlineStyle::default()),
                    ..Default::default()
                },
                InlineKind::Code => gpui::HighlightStyle {
                    background_color: Some(Self::hsla_hex(0x3b4252).opacity(0.35)),
                    ..Default::default()
                },
            };
            if mark.kind == InlineKind::Code {
                code_ranges.push((
                    mark.range.clone(),
                    gpui::SharedString::from("JetBrains Mono"),
                ));
            }
            highlights.push((mark.range.clone(), style));
        }

        let mut styled = gpui::StyledText::new(md.text.clone());
        if !code_ranges.is_empty() {
            styled = styled.with_font_family_overrides(code_ranges);
        }
        if !highlights.is_empty() {
            styled = styled.with_highlights(highlights);
        }
        styled
    }

    /// Resolve a theme hex colour string to an `Hsla` for `TextStyle` fields.
    fn hsla(hex: &str) -> gpui::Hsla {
        Self::hsla_hex(Self::color(hex))
    }

    /// Convert a 0xRRGGBB value to an `Hsla`.
    fn hsla_hex(rgb: u32) -> gpui::Hsla {
        gpui::Rgba {
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: 1.0,
        }
        .into()
    }

    /// Render the result list styled from `theme.listview` and `theme.element`.
    /// When `columns > 1`, items are laid out in a grid.
    fn render_listview(&self, cx: &mut Context<Self>, columns: usize) -> impl IntoElement {
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

        if columns > 1 {
            // Grid mode: virtualized rows of `columns` items each using uniform_list.
            let total_rows = self.filtered.len().div_ceil(columns);
            uniform_list(
                "grid_targets",
                total_rows,
                cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                    range
                        .map(|row_ix| this.render_grid_row(row_ix, columns, _cx))
                        .collect()
                }),
            )
            .track_scroll(&self.list)
            .flex_1()
            .w_full()
            .into_any()
        } else {
            // List mode: single-column vertical list with virtual scrolling.
            uniform_list(
                "targets",
                self.filtered.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                    range.map(|ix| this.render_row(ix, _cx)).collect()
                }),
            )
            .track_scroll(&self.list)
            .flex_1()
            .w_full()
            .into_any()
        }
    }

    /// Render a single row of the grid (used when `columns > 1`).
    fn render_grid_row(
        &self,
        row_ix: usize,
        cols: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let spacing = px(t.listview.spacing);
        let start_ix = row_ix * cols;
        let end_ix = (start_ix + cols).min(self.filtered.len());

        let mut row = div().flex().gap(spacing).w_full();
        for global_ix in start_ix..end_ix {
            let item = &self.filtered[global_ix];
            let is_selected = global_ix == self.selected;
            row = row.child(div().flex_1().child(self.render_grid_cell(
                item,
                is_selected,
                global_ix,
                cx,
            )));
        }
        // Pad incomplete last row to keep column alignment.
        for _ in end_ix..(start_ix + cols) {
            row = row.child(div().flex_1());
        }
        row.into_any()
    }

    /// Render a single grid cell (used when `columns > 1`).
    fn render_grid_cell(
        &self,
        item: &Target,
        is_selected: bool,
        filtered_ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let el = &t.element;

        let (cell_bg, name_color) = if is_selected {
            (
                rgb(Self::color(&el.selected.background)),
                rgb(Self::color(&el.selected.text_color)),
            )
        } else {
            (rgba(0x00000000), rgb(Self::color(&el.text_color)))
        };

        let icon_size = px(el.icon_size);
        let icon_element = if el.show_icons {
            if let Some(path) = item.icon_path() {
                img(path).w(icon_size).h(icon_size).rounded_sm().into_any()
            } else {
                let fallback = item.icon().unwrap_or("•");
                if fallback.starts_with('/') || fallback.starts_with('~') {
                    let p = expand_tilde_path(fallback);
                    img(std::path::PathBuf::from(p))
                        .w(icon_size)
                        .h(icon_size)
                        .rounded_sm()
                        .into_any()
                } else {
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
                }
            }
        } else {
            div().into_any()
        };

        let pad_h = el.padding.first().copied().unwrap_or(8.0);

        let mut cell = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .p(px(pad_h))
            .rounded(px(el.corner_radius))
            .cursor(CursorStyle::PointingHand)
            .bg(cell_bg)
            .child(icon_element)
            .child(
                div()
                    .text_color(name_color)
                    .text_size(px(t.font.size - 1.0))
                    .child(item.name().to_string()),
            );

        if el.show_category_badge {
            cell = cell.child(
                div()
                    .text_size(px(t.font.size - 2.0))
                    .text_color(rgb(Self::color(&t.listview.category_color)))
                    .child(item.category_label().to_string()),
            );
        }

        Self::with_item_mouse_handlers(cell, format!("cell-{filtered_ix}"), filtered_ix, cx)
            .into_any()
    }

    /// Build a single list row for `filtered_ix` (position within `filtered`).
    fn render_row(&self, filtered_ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
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
        let show = el.show_icons;
        let icon_element = if show {
            if let Some(path) = item.icon_path() {
                img(path).w(icon_size).h(icon_size).rounded_sm().into_any()
            } else {
                let fallback = item.icon().unwrap_or("•");
                if fallback.starts_with('/') || fallback.starts_with('~') {
                    let p = expand_tilde_path(fallback);
                    img(std::path::PathBuf::from(p))
                        .w(icon_size)
                        .h(icon_size)
                        .rounded_sm()
                        .into_any()
                } else {
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
                }
            }
        } else {
            div().into_any()
        };

        let pad_h = el.padding.first().copied().unwrap_or(8.0);
        let pad_v = el.padding.get(1).copied().unwrap_or(12.0);

        let desc_color = rgb(Self::color(
            el.description_color.as_deref().unwrap_or(&el.text_color),
        ));

        // Name column: for inline scripts show name + cached output as subtitle.
        let subtitle_opt = item.inline_output().or_else(|| item.package_name());

        let name_col = if let Some(subtitle) = subtitle_opt {
            div()
                .flex_1()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().text_color(name_color).child(item.name().to_string()))
                .child(
                    div()
                        .text_size(px(t.font.size - 2.0))
                        .text_color(desc_color)
                        .child(subtitle.to_string()),
                )
                .into_any()
        } else {
            div()
                .flex_1()
                .text_color(name_color)
                .child(item.name().to_string())
                .into_any()
        };

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
            .child(name_col);

        // Right-aligned badge with the target's bound shortcuts, if any.
        let row = match self.shortcut_label(item.name()) {
            Some(label) => row.child(
                div()
                    .text_size(px(t.font.size - 2.0))
                    .text_color(desc_color)
                    .child(label),
            ),
            None => row,
        };

        // Right-aligned category label (e.g. "Script", "Application").
        let row = if el.show_category_badge {
            let cat_color = rgb(Self::color(&t.listview.category_color));
            row.child(
                div()
                    .text_size(px(t.font.size - 2.0))
                    .text_color(cat_color)
                    .child(item.category_label().to_string()),
            )
        } else {
            row
        };
        Self::with_item_mouse_handlers(row, format!("row-{filtered_ix}"), filtered_ix, cx)
            .into_any()
    }

    /// Attach the standard list-item mouse behaviour: hovering moves the
    /// selection to the item, a left click selects and runs it. The element
    /// needs an id (stateful interactivity), so the caller supplies one.
    fn with_item_mouse_handlers(
        el: gpui::Div,
        id: String,
        filtered_ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        el.id(id)
            .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
                if *hovered
                    && matches!(this.state, LauncherState::Search)
                    && this.selected != filtered_ix
                {
                    this.selected = filtered_ix;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, event, _window, cx| {
                if is_primary_click(event) && matches!(this.state, LauncherState::Search) {
                    this.selected = filtered_ix;
                    let action = this.execute_selected();
                    this.perform_action(action, cx);
                }
            }))
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

/// Apply a full `TextStyle` to an element so its text children (including
/// `StyledText`, which inherits the parent style) render with it.
fn apply_md_style(mut el: gpui::Div, style: &gpui::TextStyle) -> gpui::Div {
    let ts = el.text_style();
    ts.color = Some(style.color);
    ts.font_size = Some(style.font_size);
    ts.font_weight = Some(style.font_weight);
    ts.font_style = Some(style.font_style);
    el
}

/// True for a left-mouse-button click (keyboard/touch-generated clicks
/// are ignored).
fn is_primary_click(event: &gpui::ClickEvent) -> bool {
    matches!(
        event,
        gpui::ClickEvent::Mouse(m) if m.down.button == gpui::MouseButton::Left
    )
}

/// Expand a leading `~` in a path to the user's home directory.
fn expand_tilde_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~')
        && let Some(home) = dirs::home_dir()
    {
        return format!("{}{rest}", home.display());
    }
    path.to_string()
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
            name: name.into(),
            mode: crate::core::item::ScriptMode::FullOutput,
            icon: None,
            path: std::sync::Arc::from(PathBuf::from(name)),
            metadata: std::sync::Arc::default(),
            metatags: crate::core::item::ScriptMetatags::default(),
            inline_output: None,
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
        assert_eq!(l.handle_keystroke(&key("escape")), LauncherAction::Hide);
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

    fn item_with_args(name: &str, n_args: usize, confirm: bool) -> Target {
        use crate::core::item::{RaycastMetadata, ScriptArgument};
        let mut metadata = RaycastMetadata::default();
        for (i, slot) in [
            &mut metadata.argument1,
            &mut metadata.argument2,
            &mut metadata.argument3,
        ]
        .iter_mut()
        .enumerate()
        .take(n_args)
        {
            **slot = Some(ScriptArgument {
                arg_type: Some("text".to_string()),
                placeholder: Some(format!("Arg {i}")),
                optional: None,
                percent_encoded: None,
                data: None,
            });
        }
        metadata.needs_confirmation = confirm.then_some(true);
        Target::Script {
            name: name.into(),
            mode: ScriptMode::Compact,
            icon: None,
            path: std::sync::Arc::from(PathBuf::from(name)),
            metadata: std::sync::Arc::new(metadata),
            metatags: crate::core::item::ScriptMetatags::default(),
            inline_output: None,
        }
    }

    fn arg_state(l: &Launcher) -> Option<(Vec<String>, usize)> {
        match &l.state {
            LauncherState::ArgumentInput {
                values,
                focused_index,
                ..
            } => Some((values.clone(), *focused_index)),
            _ => None,
        }
    }

    #[test]
    fn enter_on_script_with_args_starts_argument_prompt() {
        let mut l = Launcher::new(
            vec![item_with_args("Args Test", 2, false)],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        let action = l.handle_keystroke(&key("enter"));
        assert_eq!(action, LauncherAction::None);
        assert_eq!(arg_state(&l), Some((vec![String::new(), String::new()], 0)));
    }

    #[test]
    fn argument_prompt_accepts_typing_focus_and_confirm() {
        let mut l = Launcher::new(
            vec![item_with_args("Args Test", 2, false)],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.handle_keystroke(&key("enter")); // enter the prompt
        l.handle_keystroke(&key("h"));
        l.handle_keystroke(&key("i"));
        assert_eq!(
            arg_state(&l),
            Some((vec!["hi".to_string(), String::new()], 0))
        );
        l.handle_keystroke(&key("enter")); // advance to the next argument
        assert_eq!(
            arg_state(&l),
            Some((vec!["hi".to_string(), String::new()], 1))
        );
        l.handle_keystroke(&key("tab")); // wraps back to the first
        assert_eq!(
            arg_state(&l),
            Some((vec!["hi".to_string(), String::new()], 0))
        );
        l.handle_keystroke(&key("tab")); // back to the second
        l.handle_keystroke(&key("t"));
        l.handle_keystroke(&key("a"));
        l.handle_keystroke(&key("b"));
        l.handle_keystroke(&key("backspace")); // "ta"
        let action = l.handle_keystroke(&key("enter")); // last arg -> run
        assert!(matches!(
            action,
            LauncherAction::ExecuteScript(_, ref args)
                if *args == vec!["hi".to_string(), "ta".to_string()]
        ));
        assert_eq!(l.state, LauncherState::Search);
    }

    #[test]
    fn escape_cancels_argument_prompt() {
        let mut l = Launcher::new(
            vec![item_with_args("Args Test", 2, false)],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.handle_keystroke(&key("enter"));
        l.handle_keystroke(&key("h"));
        let action = l.handle_keystroke(&key("escape"));
        assert_eq!(action, LauncherAction::None);
        assert_eq!(l.state, LauncherState::Search);
    }

    #[test]
    fn argument_prompt_confirms_before_executing() {
        let mut l = Launcher::new(
            vec![item_with_args("Confirm Args", 1, true)],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.handle_keystroke(&key("enter"));
        l.handle_keystroke(&key("x"));
        l.handle_keystroke(&key("enter")); // last arg -> confirmation
        assert!(matches!(l.state, LauncherState::Confirming { .. }));
        l.handle_keystroke(&key("escape")); // decline
        assert_eq!(l.state, LauncherState::Search);
        l.handle_keystroke(&key("enter")); // prompt again
        l.handle_keystroke(&key("y"));
        l.handle_keystroke(&key("enter"));
        let action = l.handle_keystroke(&key("enter")); // confirm
        assert!(matches!(
            action,
            LauncherAction::ExecuteScript(_, ref args) if *args == vec!["y".to_string()]
        ));
        assert_eq!(l.state, LauncherState::Search);
    }

    #[test]
    fn full_output_still_swallows_keystrokes() {
        let mut l = Launcher::new(
            vec![item("Git Status")],
            ThemeConfig::default(),
            AppConfig::default(),
            History::test_new(PathBuf::new(), Vec::new()),
        );
        l.state = LauncherState::FullOutput { title: "t".into() };
        l.full_output_blocks = crate::core::markdown::parse("out");
        l.handle_keystroke(&key("a"));
        assert_eq!(l.query, "");
        l.handle_keystroke(&key("down"));
        assert_eq!(l.selected, 0);
        let action = l.handle_keystroke(&key("escape"));
        assert_eq!(action, LauncherAction::None);
        assert_eq!(l.state, LauncherState::Search);
    }
}
