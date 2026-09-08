//! Launcher state: struct definition and controller logic (keystroke
//! handling, filtering, execution, configuration reload).

use gpui::{Context, ScrollStrategy, UniformListScrollHandle};

use crate::core::config::AppConfig;
use crate::core::history::History;
use crate::core::item::{BuiltinAction, ScriptMetatags, ScriptMode, Target};
use crate::core::search::SearchIndex;
use crate::core::theme::ThemeConfig;
use crate::core::widget::WidgetRegistry;

use super::helpers::combo_matches;
use super::types::{LauncherAction, LauncherState};

/// Root view: renders the filter field and the ranked list of targets.
pub struct Launcher {
    /// Every indexed target, kept in name-sorted order (the "unfiltered" order).
    pub(super) all: Vec<Target>,
    /// The ranked results for the current query (best first), capped at
    /// `max_results`.
    pub(super) filtered: Vec<Target>,
    /// Current filter query.
    pub(super) query: String,
    /// Position of the highlighted row within `filtered`.
    pub(super) selected: usize,
    /// Scroll state of the targets list (wheel scrolling + arrow auto-scroll).
    pub(super) list: UniformListScrollHandle,
    /// Reused nucleo matcher (it allocates a working set up front).
    pub(super) search: SearchIndex,
    /// Launch history, appended to on every execution (frecency source).
    pub(super) history: History,
    /// The app configuration this launcher was built from (re-read by
    /// "Reload Configuration").
    pub(super) app_config: AppConfig,
    /// Active theme controlling every visual aspect of the launcher.
    pub(super) theme: ThemeConfig,
    /// Current state of the launcher (e.g. normal search or showing script output).
    pub(super) state: LauncherState,
    /// Scroll state for fullOutput mode.
    pub(super) full_output_scroll: UniformListScrollHandle,
    /// Parsed markdown blocks of the current full-output result (empty
    /// while not showing one; cleared on hide to release the memory).
    pub(super) full_output_blocks: Vec<crate::core::markdown::MdBlock>,
    /// Layout overrides (`@aerofi.show_search` / `@aerofi.columns`)
    /// committed by the last executed script. Kept until the launcher is
    /// hidden; never applied by selection alone.
    pub(super) sticky_metatags: Option<ScriptMetatags>,
    /// Registry of custom widget definitions from `[[widgets]]` in the
    /// theme file. Used by `render_custom_widget` in `custom_widgets.rs`.
    pub(super) widget_registry: WidgetRegistry,
}

impl Launcher {
    pub fn new(
        all: Vec<Target>,
        theme: ThemeConfig,
        app_config: AppConfig,
        history: History,
    ) -> Self {
        let filtered = all.clone();
        let widget_registry = WidgetRegistry::from_theme(&theme.widgets);
        Self {
            all,
            filtered,
            query: String::new(),
            selected: 0,
            list: UniformListScrollHandle::new(),
            search: SearchIndex::new(&app_config.aliases),
            history,
            app_config,
            theme,
            state: LauncherState::Search,
            full_output_scroll: UniformListScrollHandle::new(),
            full_output_blocks: Vec::new(),
            sticky_metatags: None,
            widget_registry,
        }
    }

    /// Effective list columns: the sticky metatag override from the last
    /// executed script, or the theme's configured value.
    pub(super) fn effective_columns(&self) -> usize {
        self.sticky_metatags
            .as_ref()
            .and_then(|m| m.columns)
            .unwrap_or(self.theme.listview.columns)
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
                    && values[*focused_index].pop().is_none()
                    && *focused_index > 0
                {
                    *focused_index -= 1;
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
                    && let LauncherState::ArgumentInput {
                        values,
                        focused_index,
                        ..
                    } = &mut self.state
                {
                    values[*focused_index].push_str(c);
                }
                LauncherAction::None
            }
            _ if !matches!(self.state, LauncherState::Search) => LauncherAction::None,
            ("up", false) => {
                let cols = self.effective_columns();
                let step = if cols > 1 { cols as isize } else { 1 };
                self.move_selection(-step);
                LauncherAction::None
            }
            ("down", false) => {
                let cols = self.effective_columns();
                let step = if cols > 1 { cols as isize } else { 1 };
                self.move_selection(step);
                LauncherAction::None
            }
            ("left", false) if self.effective_columns() > 1 => {
                self.move_selection(-1);
                LauncherAction::None
            }
            ("right", false) if self.effective_columns() > 1 => {
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
        let cols = self.effective_columns();
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
        // The sticky layout override only lasts for the session.
        self.sticky_metatags = None;
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
    pub(super) fn alias_target(&self) -> Option<&Target> {
        let name = self.app_config.aliases.get(&self.query)?;
        self.all.iter().find(|t| t.name() == name)
    }

    /// Run the highlighted target.
    pub(super) fn execute_selected(&mut self) -> LauncherAction {
        let Some(item) = self.selected_item() else {
            return LauncherAction::None;
        };
        let item = item.clone();
        self.execute_item(&item)
    }

    /// Run a target: built-in actions act in place, apps open and scripts
    /// run asynchronously via `LauncherAction::ExecuteScript`.
    pub(super) fn execute_item(&mut self, item: &Target) -> LauncherAction {
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

    pub(super) fn execute_target_with_args(
        &mut self,
        item: &Target,
        args: Vec<String>,
    ) -> LauncherAction {
        // Commit the script's layout metatags: execution, not selection,
        // makes the override stick.
        self.sticky_metatags = item.metatags().cloned();
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
        // Update the master list and the rendered rows in place. Inline
        // output never affects ranking, so re-filtering (which resets the
        // scroll position) is unnecessary.
        for target in self.all.iter_mut().chain(self.filtered.iter_mut()) {
            let is_match = matches!(
                target,
                Target::Script {
                    path: p,
                    mode: ScriptMode::Inline,
                    ..
                } if p.as_ref() == path
            );
            if is_match {
                target.set_inline_output(output.clone());
            }
        }
    }

    /// Re-read `config.toml`, rescan the targets and rebuild the search
    /// index (aliases, `max_results`, sources, ignored apps, script dirs).
    fn reload(&mut self) {
        let config = crate::core::config::AppConfig::load();
        let theme_name = &config.theme;
        let theme = crate::core::theme::load_theme(theme_name);
        let targets = crate::core::scanner::scan_all(&config);
        self.app_config = config;
        self.all = targets;
        self.search = SearchIndex::new(&self.app_config.aliases);
        self.widget_registry = WidgetRegistry::from_theme(&theme.widgets);
        self.theme = theme;
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

    /// Execute an action triggered by a custom button widget.
    pub(super) fn handle_widget_button_action(&mut self, action: &str, cx: &mut Context<Self>) {
        let trimmed = action.trim();
        if trimmed.eq_ignore_ascii_case("reload") {
            self.reload();
            cx.notify();
            return;
        }
        if trimmed.eq_ignore_ascii_case("hide") {
            self.on_hide();
            cx.notify();
            crate::ui::window::hide();
            return;
        }
        if let Some(cmd) = trimmed.strip_prefix("command:") {
            let cmd = cmd.trim().to_string();
            std::thread::spawn(move || {
                let _ = std::process::Command::new("sh").arg("-c").arg(cmd).spawn();
            });
            return;
        }

        let target_name = trimmed
            .strip_prefix("target:")
            .or_else(|| trimmed.strip_prefix("run:"))
            .or_else(|| trimmed.strip_prefix("script:"))
            .unwrap_or(trimmed);

        if let Some(target) = self.all.iter().find(|t| t.name() == target_name).cloned() {
            let action = self.execute_item(&target);
            self.perform_action(action, cx);
            cx.notify();
        } else {
            eprintln!("aerofi: warning: button target not found: {target_name}");
        }
    }
}
