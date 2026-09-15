//! Launcher state: struct definition and controller logic (keystroke
//! handling, filtering, execution, configuration reload).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gpui::{Context, ScrollStrategy, UniformListScrollHandle};

use crate::core::config::AppConfig;
use crate::core::gui_protocol::GuiCommand;
use crate::core::gui_session::{self, GuiSession, ReadResult};
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
    /// `max_results`. Stores indices into `all`.
    pub(super) filtered: Vec<usize>,
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
    /// Scroll state for the GUI-mode rows list (kept separate from the
    /// search list so each mode's scroll position is independent).
    pub(super) gui_rows_scroll: UniformListScrollHandle,
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
    /// Hotkey bindings from button widgets: maps combo string (e.g. "cmd+r")
    /// to the button's action string. Rebuilt on config reload.
    pub(super) button_hotkeys: HashMap<String, String>,
    /// Active GUI-mode script session (stdin/stdout pipe to child).
    /// Wrapped in Arc<Mutex<>> so it can be shared with async tasks.
    pub(super) gui_session: Option<Arc<Mutex<GuiSession>>>,
    /// Manager for dynamic `.dylib` plugins.
    pub(super) plugin_manager: crate::core::plugin_manager::PluginManager,
    /// Active plugin search task.
    pub(super) plugin_search_task: Option<gpui::Task<()>>,
    /// Number of base items (apps/scripts/builtins). Plugin items are appended after this.
    pub(super) base_count: usize,
    /// Set to `true` after a config/theme reload so the next `render()` call
    /// re-centers the window *after* `window.resize()` has been applied.
    pub(super) needs_center: bool,
}

impl Launcher {
    pub fn new(
        all: Vec<Target>,
        theme: ThemeConfig,
        app_config: AppConfig,
        history: History,
    ) -> Self {
        let filtered = (0..all.len()).collect();
        let widget_registry = WidgetRegistry::from_theme(&theme.widgets);
        let button_hotkeys = widget_registry.button_hotkeys();
        let base_count = all.len();
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
            gui_rows_scroll: UniformListScrollHandle::new(),
            full_output_blocks: Vec::new(),
            sticky_metatags: None,
            widget_registry,
            button_hotkeys,
            gui_session: None,
            plugin_manager: crate::core::plugin_manager::PluginManager::load_all(),
            plugin_search_task: None,
            base_count,
            needs_center: false,
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
    pub fn handle_keystroke(
        &mut self,
        ks: &gpui::Keystroke,
        cx: Option<&mut Context<Self>>,
    ) -> LauncherAction {
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

        // GUI-mode interactive script: handle keystrokes within the
        // script-driven list.
        if matches!(&self.state, LauncherState::GuiMode { .. }) {
            return self.handle_gui_mode_keystroke(ks, cx);
        }

        // A configured key-combo shortcut (e.g. "cmd+r") runs its target
        // immediately; explicit config overrides the built-in bindings.
        // Only honoured in plain search mode — while an argument prompt or
        // confirmation is up, every keystroke belongs to that prompt.
        if matches!(self.state, LauncherState::Search)
            && let Some((_, name)) = self
                .app_config
                .bindings
                .launcher
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

        // Button widget hotkeys (e.g. "cmd+r" mapped to a sidebar button's
        // action). Only honoured in plain search mode.
        if matches!(self.state, LauncherState::Search)
            && let Some((_, action)) = self
                .button_hotkeys
                .iter()
                .find(|(combo, _)| combo_matches(combo, ks))
        {
            let action = action.clone();
            if let Some(cx) = cx {
                self.handle_widget_button_action(&action, None, cx);
            }
            return LauncherAction::None;
        }

        // Reload configuration (customizable via [general] reload_hotkey or built-in Cmd+R / Ctrl+R)
        if self.is_reload_keystroke(ks) {
            self.reload();
            if let Some(cx) = cx {
                cx.notify();
            }
            return LauncherAction::None;
        }

        let cmd = ks.modifiers.platform;
        let ctrl = ks.modifiers.control;
        let alt = ks.modifiers.alt;

        // Common text editing and navigation hotkeys
        if matches!(self.state, LauncherState::Search) {
            match (ks.key.as_str(), cmd, ctrl, alt) {
                ("u", false, true, false) | ("backspace", true, false, false) => {
                    self.reset();
                    return LauncherAction::None;
                }
                ("w", false, true, false) | ("backspace", false, false, true) => {
                    let len = self.query.trim_end().rfind(' ').map(|i| i + 1).unwrap_or(0);
                    self.query.truncate(len);
                    self.refilter(cx);
                    self.selected = 0;
                    return LauncherAction::None;
                }
                ("n", false, true, false) | ("j", false, true, false) => {
                    let cols = self.effective_columns();
                    let step = if cols > 1 { cols as isize } else { 1 };
                    self.move_selection(step);
                    return LauncherAction::None;
                }
                ("p", false, true, false) | ("k", false, true, false) => {
                    let cols = self.effective_columns();
                    let step = if cols > 1 { cols as isize } else { 1 };
                    self.move_selection(-step);
                    return LauncherAction::None;
                }
                _ => {}
            }
        }

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
                let shift = ks.modifiers.shift;
                if let LauncherState::ArgumentInput {
                    args,
                    focused_index,
                    ..
                } = &mut self.state
                {
                    if shift {
                        *focused_index = (*focused_index + args.len() - 1) % args.len();
                    } else {
                        *focused_index = (*focused_index + 1) % args.len();
                    }
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
            ("up", true) => {
                if self.filtered.is_empty() {
                    return LauncherAction::None;
                }
                self.selected = 0;
                LauncherAction::None
            }
            ("down", true) => {
                if self.filtered.is_empty() {
                    return LauncherAction::None;
                }
                self.selected = self.filtered.len() - 1;
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
            ("backspace", false) => {
                self.backspace(cx);
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
                    self.refilter(cx);
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
        self.refilter(None);
        self.selected = 0;
    }

    fn backspace(&mut self, cx: Option<&mut Context<Self>>) {
        if self.query.pop().is_some() {
            self.refilter(cx);
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
    fn refilter(&mut self, cx: Option<&mut Context<Self>>) {
        // Clear any previous plugin items
        self.all.truncate(self.base_count);

        if let Some((plugin, remainder)) = self.plugin_manager.match_prefix(&self.query) {
            let remainder = remainder.to_string();

            if let Some(cx) = cx {
                // Show base app/script matches until the debounced plugin
                // results arrive, so `filtered` never points at truncated
                // rows in the meantime.
                self.search.search(
                    &self.query,
                    &self.all[..self.base_count],
                    &self.history,
                    &mut self.filtered,
                );

                // Every keystroke re-enters this branch and drops the
                // previous task, so the plugin is queried at most once per
                // typing pause instead of spawning a process per key.
                self.plugin_search_task = Some(cx.spawn(
                    |view: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                        // Clone into an owned handle so the future is 'static.
                        let mut cx = cx.clone();
                        async move {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(90))
                                .await;
                            // The query may have changed (or the view been dropped)
                            // while the debounce was pending.
                            let Some((plugin, remainder)) = view
                                .update(&mut cx, |this, _| {
                                    this.plugin_manager
                                        .match_prefix(&this.query)
                                        .map(|(p, r)| (p, r.to_string()))
                                })
                                .ok()
                                .flatten()
                            else {
                                return;
                            };

                            let (tx, rx) = futures::channel::oneshot::channel();
                            let plugin_clone = plugin.clone();
                            let remainder_str = remainder;

                            std::thread::Builder::new()
                                .name("aerofi-plugin-search".into())
                                .spawn(move || {
                                    let results = plugin_clone.query(&remainder_str);

                                    let items = if results.count == 0 || results.items.is_null() {
                                        &[]
                                    } else {
                                        unsafe {
                                            std::slice::from_raw_parts(results.items, results.count)
                                        }
                                    };

                                    let mut parsed = Vec::with_capacity(results.count);
                                    for item in items {
                                        let name = if item.title.is_null() {
                                            gpui::SharedString::from("")
                                        } else {
                                            let c_str =
                                                unsafe { std::ffi::CStr::from_ptr(item.title) };
                                            gpui::SharedString::from(
                                                c_str.to_string_lossy().into_owned(),
                                            )
                                        };

                                        let subtitle = if item.subtitle.is_null() {
                                            None
                                        } else {
                                            let c_str =
                                                unsafe { std::ffi::CStr::from_ptr(item.subtitle) };
                                            Some(gpui::SharedString::from(
                                                c_str.to_string_lossy().into_owned(),
                                            ))
                                        };

                                        let icon = if item.icon.is_null() {
                                            None
                                        } else {
                                            let c_str =
                                                unsafe { std::ffi::CStr::from_ptr(item.icon) };
                                            Some(gpui::SharedString::from(
                                                c_str.to_string_lossy().into_owned(),
                                            ))
                                        };

                                        let plugin_id = if item.id.is_null() {
                                            gpui::SharedString::from("")
                                        } else {
                                            let c_str =
                                                unsafe { std::ffi::CStr::from_ptr(item.id) };
                                            gpui::SharedString::from(
                                                c_str.to_string_lossy().into_owned(),
                                            )
                                        };

                                        parsed.push(Target::PluginItem {
                                            name,
                                            subtitle,
                                            icon,
                                            plugin_id,
                                            plugin_name: gpui::SharedString::from(
                                                plugin.name.clone(),
                                            ),
                                        });
                                    }

                                    plugin.free_results(results);
                                    let _ = tx.send(parsed);
                                })
                                .unwrap();

                            if let Ok(parsed_items) = rx.await {
                                let _ = view.update(&mut cx, |this, cx| {
                                    this.all.truncate(this.base_count);
                                    this.all.extend(parsed_items);
                                    this.filtered = (this.base_count..this.all.len()).collect();
                                    if this.selected >= this.filtered.len() {
                                        this.selected = 0;
                                    }
                                    this.list.scroll_to_item(0, ScrollStrategy::Top);
                                    cx.notify();
                                });
                            }
                        }
                    },
                ));
            } else {
                let results = plugin.query(&remainder);

                // Convert C ABI results to Rust Targets
                let items = if results.count == 0 || results.items.is_null() {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(results.items, results.count) }
                };

                for item in items {
                    let name = if item.title.is_null() {
                        gpui::SharedString::from("")
                    } else {
                        let c_str = unsafe { std::ffi::CStr::from_ptr(item.title) };
                        gpui::SharedString::from(c_str.to_string_lossy().into_owned())
                    };

                    let subtitle = if item.subtitle.is_null() {
                        None
                    } else {
                        let c_str = unsafe { std::ffi::CStr::from_ptr(item.subtitle) };
                        Some(gpui::SharedString::from(
                            c_str.to_string_lossy().into_owned(),
                        ))
                    };

                    let icon = if item.icon.is_null() {
                        None
                    } else {
                        let c_str = unsafe { std::ffi::CStr::from_ptr(item.icon) };
                        Some(gpui::SharedString::from(
                            c_str.to_string_lossy().into_owned(),
                        ))
                    };

                    let plugin_id = if item.id.is_null() {
                        gpui::SharedString::from("")
                    } else {
                        let c_str = unsafe { std::ffi::CStr::from_ptr(item.id) };
                        gpui::SharedString::from(c_str.to_string_lossy().into_owned())
                    };

                    self.all.push(Target::PluginItem {
                        name,
                        subtitle,
                        icon,
                        plugin_id,
                        plugin_name: gpui::SharedString::from(plugin.name.clone()),
                    });
                }

                plugin.free_results(results);

                // For plugins, we don't fuzzy sort. We show exactly what the plugin returned in order.
                self.filtered = (self.base_count..self.all.len()).collect();
            }
        } else {
            self.search.search(
                &self.query,
                &self.all[..self.base_count],
                &self.history,
                &mut self.filtered,
            );
        }

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
        // Kill any active GUI session.
        if let Some(session) = self.gui_session.take()
            && let Ok(mut s) = session.lock()
        {
            s.kill();
        }
    }

    /// Called when the window is shown.  Refills `filtered` from `all`
    /// so the next render creates fresh `img()` elements that GPUI will
    /// decode on demand.
    pub fn on_show(&mut self) {
        self.refilter(None);
        // refilter() resets the scroll to the top, but the selection
        // survived the hide/show cycle — restore the view to it so the
        // user doesn't lose their place.
        self.list
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
    }

    pub fn selected_item(&self) -> Option<&Target> {
        self.filtered.get(self.selected).map(|&i| &self.all[i])
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
            Target::PluginItem {
                plugin_name,
                plugin_id,
                ..
            } => {
                // For plugins, we call activate via the plugin manager.
                // We'll return an action that handles this.
                LauncherAction::ActivatePlugin {
                    plugin_name: plugin_name.to_string(),
                    plugin_id: plugin_id.to_string(),
                    action_code: 0,
                }
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
                    ScriptMode::Gui => LauncherAction::StartGuiSession(item.clone(), args),
                }
            }
            Target::Builtin { .. } | Target::App { .. } => LauncherAction::None,
            Target::PluginItem {
                plugin_name,
                plugin_id,
                ..
            } => LauncherAction::ActivatePlugin {
                plugin_name: plugin_name.to_string(),
                plugin_id: plugin_id.to_string(),
                action_code: 0,
            },
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
            LauncherAction::StartGuiSession(target, args) => {
                self.start_gui_session(&target, args, cx);
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
            LauncherAction::ActivatePlugin {
                plugin_name,
                plugin_id,
                action_code,
            } => {
                if let Some(plugin) = self
                    .plugin_manager
                    .plugins
                    .iter()
                    .find(|p| p.name == plugin_name)
                    && plugin.activate(&plugin_id, action_code)
                {
                    self.on_hide();
                    cx.notify();
                    crate::ui::window::hide();
                }
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
        for target in self.all.iter_mut() {
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
        let mut targets = crate::core::scanner::scan_all(&config);
        crate::sys::icons::extract_all(&mut targets);
        self.app_config = config;
        self.base_count = targets.len();
        self.all = targets;
        self.search = SearchIndex::new(&self.app_config.aliases);
        self.plugin_manager = crate::core::plugin_manager::PluginManager::load_all();
        self.widget_registry = WidgetRegistry::from_theme(&theme.widgets);
        self.button_hotkeys = self.widget_registry.button_hotkeys();
        self.theme = theme;
        crate::sys::appkit::set_corner_radius(self.theme.window.corner_radius);
        // Defer centering to the next render() call so it fires *after*
        // window.resize() applies the new theme dimensions.
        self.needs_center = true;
        self.query.clear();
        self.refilter(None);
        self.selected = 0;
        println!(
            "aerofi: configuration reloaded (theme: {})",
            self.app_config.theme
        );
    }

    /// Check if the keystroke triggers configuration reload.
    fn is_reload_keystroke(&self, ks: &gpui::Keystroke) -> bool {
        let is_bound = self
            .app_config
            .bindings
            .launcher
            .iter()
            .any(|(combo, name)| name == "Reload Configuration" && combo_matches(combo, ks));

        if is_bound {
            return true;
        }

        let cmd = ks.modifiers.platform;
        let ctrl = ks.modifiers.control;
        (cmd || ctrl) && (ks.key.eq_ignore_ascii_case("r") || ks.key == "к" || ks.key == "К")
    }

    /// Execute an action triggered by a custom button widget. If rendered
    /// inside a list item row, `row_item` is provided to allow contextual
    /// actions like "run", "edit", or "copy".
    pub(super) fn handle_widget_button_action(
        &mut self,
        action: &str,
        row_item: Option<&Target>,
        cx: &mut Context<Self>,
    ) {
        let trimmed = action.trim();

        // Row-context actions
        if trimmed.eq_ignore_ascii_case("run") || trimmed.eq_ignore_ascii_case("execute") {
            if let Some(item) = row_item.cloned() {
                let action = self.execute_item(&item);
                self.perform_action(action, cx);
                cx.notify();
            }
            return;
        }

        if trimmed.eq_ignore_ascii_case("copy") {
            if let Some(item) = row_item {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(item.name().to_string()));
            }
            return;
        }

        // Global actions
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
            let _ = std::thread::Builder::new()
                .name("aerofi-cmd".to_string())
                .stack_size(128 * 1024)
                .spawn(move || {
                    if let Ok(mut child) =
                        std::process::Command::new("sh").arg("-c").arg(cmd).spawn()
                    {
                        let _ = child.wait();
                    }
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

    // ── GUI-mode interactive script session ────────────────────────────

    /// Spawn a GUI session for an interactive script and read its initial
    /// burst of output.  Transitions to `LauncherState::GuiMode`.
    fn start_gui_session(&mut self, target: &Target, args: Vec<String>, cx: &mut Context<Self>) {
        let Target::Script { path, name, .. } = target else {
            return;
        };
        let title = name.to_string();

        // Commit metatags (same as other modes).
        self.sticky_metatags = target.metatags().cloned();

        // Record in history.
        let identifier = target.identifier();
        self.history.record_launch(identifier);

        let path = path.clone();
        let mut envs = std::collections::HashMap::new();
        envs.insert("AEROFI_RETV".to_string(), "0".to_string());

        match GuiSession::spawn(&path, args, envs) {
            Ok(session) => {
                let session = Arc::new(Mutex::new(session));
                self.gui_session = Some(session.clone());

                // Read initial burst on a background thread, then update UI.
                let view = cx.entity();
                let title2 = title.clone();
                let cx_async = cx.to_async();
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = std::thread::Builder::new()
                    .name("aerofi-gui-init".to_string())
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        let burst = {
                            let session = session.lock().unwrap();
                            session.read_burst(std::time::Duration::from_secs(5))
                        };
                        let _ = tx.send(burst);
                    });
                cx.spawn(move |_, _: &mut gpui::AsyncApp| async move {
                    if let Ok(burst) = rx.await {
                        cx_async.update(|cx| {
                            view.update(cx, |launcher, cx| {
                                launcher.gui_handle_read_result(burst, &title2);
                                cx.notify();
                            });
                        });
                    }
                })
                .detach();

                // Set a temporary "loading" GUI mode while waiting for the burst.
                self.state = LauncherState::GuiMode {
                    title,
                    rows: Vec::new(),
                    filtered_rows: Vec::new(),
                    prompt: None,
                    message: Some("Loading…".to_string()),
                    no_custom: false,
                    selected: 0,
                    columns: None,
                    query: String::new(),
                    loading: true,
                    live_search: false,
                    active_indices: Vec::new(),
                    data: None,
                    preview_blocks: None,
                    multi_select: false,
                    toggled_indices: std::collections::HashSet::new(),
                    markup_rows: false,
                };
            }
            Err(e) => {
                eprintln!("aerofi: failed to start GUI session for {title}: {e}");
            }
        }
    }

    /// Handle the read result from a GUI session burst.
    fn gui_handle_read_result(&mut self, result: ReadResult, title: &str) {
        match result {
            ReadResult::Burst(burst) => {
                self.gui_apply_burst(burst, title);
            }
            ReadResult::BurstThenExit(burst) => {
                self.gui_apply_burst(burst, title);
                // Script exited after this burst — mark session as done.
                // The UI will stay showing the last rows but selecting will
                // leave GUI mode.
                self.gui_session = None;
            }
            ReadResult::Exited => {
                // Script exited with no output — return to search.
                self.gui_leave();
            }
            ReadResult::Error(e) => {
                eprintln!("aerofi: GUI script error: {e}");
                self.gui_leave();
            }
        }
    }

    /// Apply a GUI burst (commands + rows) to the current GuiMode state.
    fn gui_apply_burst(&mut self, burst: crate::core::gui_protocol::GuiBurst, title: &str) {
        let mut prompt = None;
        let mut message = None;
        let mut no_custom = false;
        let mut keep_selection: Option<String> = None;
        let mut columns = None;
        let mut loading = false;
        let mut live_search = false;
        let mut active_indices = Vec::new();
        let mut data = None;
        let mut preview_blocks = None;
        let mut multi_select = false;
        let mut toggled_indices = std::collections::HashSet::new();
        let mut markup_rows = false;

        // Inherit flags if we are updating an existing GuiMode session
        if let LauncherState::GuiMode {
            prompt: prev_prompt,
            no_custom: prev_no_custom,
            columns: prev_columns,
            live_search: prev_live_search,
            active_indices: prev_active_indices,
            data: prev_data,
            preview_blocks: prev_preview_blocks,
            multi_select: prev_multi_select,
            toggled_indices: prev_toggled_indices,
            markup_rows: prev_markup_rows,
            ..
        } = &self.state
        {
            prompt = prev_prompt.clone();
            no_custom = *prev_no_custom;
            columns = *prev_columns;
            live_search = *prev_live_search;
            active_indices = prev_active_indices.clone();
            data = prev_data.clone();
            preview_blocks = prev_preview_blocks.clone();
            multi_select = *prev_multi_select;
            toggled_indices = prev_toggled_indices.clone();
            markup_rows = *prev_markup_rows;
        }

        for cmd in &burst.commands {
            match cmd {
                GuiCommand::Flush => {}
                GuiCommand::SetPrompt(p) => prompt = Some(p.clone()),
                GuiCommand::SetMessage(m) => message = Some(m.clone()),
                GuiCommand::EnableMarkup => markup_rows = true,
                GuiCommand::NoCustom(v) => no_custom = *v,
                GuiCommand::KeepSelection(s) => keep_selection = Some(s.clone()),
                GuiCommand::SetColumns(n) => columns = Some(*n),
                GuiCommand::SetLoading(l) => loading = *l,
                GuiCommand::LiveSearch(ls) => live_search = *ls,
                GuiCommand::SetActiveIndices(indices) => active_indices = indices.clone(),
                GuiCommand::SetData(d) => data = Some(d.clone()),
                GuiCommand::PreviewText(text) => {
                    preview_blocks = Some(crate::core::markdown::parse(text));
                }
                GuiCommand::PreviewFile(file_path) => {
                    let resolved = super::helpers::expand_tilde_path(file_path);
                    if let Ok(text) = std::fs::read_to_string(&resolved) {
                        preview_blocks = Some(crate::core::markdown::parse(&text));
                    }
                }
                GuiCommand::MultiSelect(b) => multi_select = *b,
                GuiCommand::MarkupRows(b) => markup_rows = *b,
                GuiCommand::Reload => {
                    // Leave GUI mode before reloading so the user sees the
                    // regular search list immediately after the script exits.
                    // We must return early to avoid the `self.state = GuiMode`
                    // assignment at the bottom of this function overwriting the
                    // `Search` state that gui_leave() sets.
                    self.gui_leave();
                    self.reload();
                    return;
                }
            }
        }

        let row_count = burst.rows.len();
        let filtered_rows: Vec<usize> = (0..row_count).collect();

        // Find pre-selected row if requested.
        let selected = keep_selection
            .and_then(|sel| burst.rows.iter().position(|r| r.text == sel))
            .unwrap_or(0);

        self.state = LauncherState::GuiMode {
            title: title.to_string(),
            rows: burst.rows,
            filtered_rows,
            prompt,
            message,
            no_custom,
            selected,
            columns,
            query: String::new(),
            loading,
            live_search,
            active_indices,
            data,
            preview_blocks,
            multi_select,
            toggled_indices,
            markup_rows,
        };
    }

    /// Handle keystrokes while in `LauncherState::GuiMode`.
    fn handle_gui_mode_keystroke(
        &mut self,
        ks: &gpui::Keystroke,
        cx: Option<&mut Context<Self>>,
    ) -> LauncherAction {
        let cmd = ks.modifiers.platform;
        let ctrl = ks.modifiers.control;
        let alt = ks.modifiers.alt;
        let shift = ks.modifiers.shift;

        let mut custom_retv = None;
        for (kb, combo_str) in &self.app_config.bindings.custom {
            if combo_matches(combo_str, ks)
                && let Some(num_str) = kb.strip_prefix("kb-custom-")
                && let Ok(num) = num_str.parse::<i32>()
                && (1..=19).contains(&num)
            {
                custom_retv = Some(num + 9);
                break;
            }
        }

        if let Some(retv) = custom_retv {
            if let Some(cx) = cx {
                self.gui_dispatch_selection(cx, "custom", Some(retv));
            }
            return LauncherAction::None;
        }

        if self.is_reload_keystroke(ks) {
            self.reload();
            if let Some(cx) = cx {
                cx.notify();
            }
            return LauncherAction::None;
        }

        match (ks.key.as_str(), cmd, ctrl, alt, shift) {
            ("escape", false, false, false, false) => {
                self.gui_leave();
                LauncherAction::None
            }
            ("enter" | "return", false, false, false, false) => {
                if let Some(cx) = cx {
                    self.gui_select_row(cx);
                }
                LauncherAction::None
            }
            ("enter" | "return", false, false, false, true) => {
                // Shift+Enter contextual action
                if let Some(cx) = cx {
                    self.gui_action_row(cx, "shift+enter");
                }
                LauncherAction::None
            }
            ("enter" | "return", false, false, true, false) => {
                // Alt+Enter contextual action
                if let Some(cx) = cx {
                    self.gui_action_row(cx, "alt+enter");
                }
                LauncherAction::None
            }
            ("up", false, false, false, false)
            | ("p", false, true, false, false)
            | ("k", false, true, false, false) => {
                let cols = self.gui_columns() as isize;
                self.gui_move_selection(-cols);
                LauncherAction::None
            }
            ("down", false, false, false, false)
            | ("n", false, true, false, false)
            | ("j", false, true, false, false) => {
                let cols = self.gui_columns() as isize;
                self.gui_move_selection(cols);
                LauncherAction::None
            }
            ("up", true, false, false, false) => {
                self.gui_jump_to_edge(true);
                LauncherAction::None
            }
            ("down", true, false, false, false) => {
                self.gui_jump_to_edge(false);
                LauncherAction::None
            }
            ("left", false, false, false, false) | ("b", false, true, false, false) => {
                self.gui_move_selection(-1);
                LauncherAction::None
            }
            ("right", false, false, false, false) | ("f", false, true, false, false) => {
                self.gui_move_selection(1);
                LauncherAction::None
            }
            ("tab", false, false, false, false) => {
                self.gui_toggle_selection(1);
                LauncherAction::None
            }
            ("tab", false, false, false, true) => {
                self.gui_toggle_selection(-1);
                LauncherAction::None
            }
            ("backspace", false, false, false, false) => {
                let mut is_live = false;
                if let LauncherState::GuiMode {
                    query, live_search, ..
                } = &mut self.state
                {
                    is_live = *live_search;
                    if query.pop().is_some() && !is_live {
                        self.gui_refilter();
                    }
                }
                if is_live && let Some(cx) = cx {
                    self.gui_send_live_search(cx);
                }
                LauncherAction::None
            }
            (key, false, true, false, false) => {
                // Ctrl + <key> contextual action (e.g. ctrl+e, ctrl+d)
                if let Some(cx) = cx {
                    let action_key = format!("ctrl+{key}");
                    self.gui_action_row(cx, &action_key);
                }
                LauncherAction::None
            }
            _ => {
                if !cmd
                    && !ctrl
                    && !alt
                    && ks.key != "tab"
                    && let Some(c) = ks.key_char.as_deref()
                    && !c.is_empty()
                    && !c.chars().any(char::is_control)
                {
                    let mut is_live = false;
                    if let LauncherState::GuiMode {
                        query, live_search, ..
                    } = &mut self.state
                    {
                        query.push_str(c);
                        is_live = *live_search;
                        if !is_live {
                            self.gui_refilter();
                        }
                    }
                    if is_live && let Some(cx) = cx {
                        self.gui_send_live_search(cx);
                    }
                }
                LauncherAction::None
            }
        }
    }

    /// Move the selection cursor within the filtered GUI rows.
    fn gui_move_selection(&mut self, delta: isize) {
        let new_selected = if let LauncherState::GuiMode {
            filtered_rows,
            selected,
            ..
        } = &mut self.state
        {
            if filtered_rows.is_empty() {
                return;
            }
            let len = filtered_rows.len() as isize;
            *selected = (*selected as isize + delta).clamp(0, len - 1) as usize;
            *selected
        } else {
            return;
        };
        // Keep the selected row in view as the user navigates with the arrows.
        // In grid mode the list items are rows of `cols` cells, so convert
        // the flat selection index to the grid-row index first.
        let cols = self.gui_columns().max(1);
        let item_ix = if cols > 1 {
            new_selected / cols
        } else {
            new_selected
        };
        self.gui_rows_scroll
            .scroll_to_item(item_ix, ScrollStrategy::Nearest);
    }

    fn gui_jump_to_edge(&mut self, top: bool) {
        let new_selected = if let LauncherState::GuiMode {
            filtered_rows,
            selected,
            ..
        } = &mut self.state
        {
            if filtered_rows.is_empty() {
                return;
            }
            *selected = if top { 0 } else { filtered_rows.len() - 1 };
            *selected
        } else {
            return;
        };
        let cols = self.gui_columns().max(1);
        let item_ix = if cols > 1 {
            new_selected / cols
        } else {
            new_selected
        };
        self.gui_rows_scroll
            .scroll_to_item(item_ix, ScrollStrategy::Nearest);
    }

    /// Toggle selection of the currently focused row if multi-select is enabled.
    fn gui_toggle_selection(&mut self, delta: isize) {
        let selected_copy = if let LauncherState::GuiMode {
            filtered_rows,
            selected,
            multi_select,
            toggled_indices,
            ..
        } = &mut self.state
        {
            if !*multi_select || filtered_rows.is_empty() {
                // If not multi_select, tab does nothing but we could just let it move selection?
                // For now just return.
                return;
            }
            let row_idx = filtered_rows[*selected];
            if toggled_indices.contains(&row_idx) {
                toggled_indices.remove(&row_idx);
            } else {
                toggled_indices.insert(row_idx);
            }
            Some(*selected)
        } else {
            None
        };

        if selected_copy.is_some() {
            self.gui_move_selection(delta);
        }
    }

    pub(super) fn gui_columns(&self) -> usize {
        if let LauncherState::GuiMode { columns, .. } = &self.state {
            (*columns).unwrap_or(1)
        } else {
            1
        }
    }

    /// User selected a row in GUI mode with standard Enter.
    pub(super) fn gui_select_row(&mut self, cx: &mut Context<Self>) {
        self.gui_dispatch_selection(cx, "enter", None);
    }

    /// User triggered a contextual action key on the selected row in GUI mode.
    pub(super) fn gui_action_row(&mut self, cx: &mut Context<Self>, key: &str) {
        self.gui_dispatch_selection(cx, key, Some(10));
    }

    /// Dispatch either a Select, Action, or Custom input event to the GUI script's stdin.
    fn gui_dispatch_selection(
        &mut self,
        cx: &mut Context<Self>,
        key: &str,
        action_retv: Option<i32>,
    ) {
        let LauncherState::GuiMode {
            rows,
            filtered_rows,
            selected,
            title,
            no_custom,
            query,
            data,
            toggled_indices,
            ..
        } = &self.state
        else {
            return;
        };

        let title = title.clone();
        let retv = action_retv.unwrap_or(if key == "enter" { 1 } else { 10 });
        let is_action = action_retv.is_some() || key != "enter";

        let event = if let Some(&row_idx) = filtered_rows.get(*selected) {
            let row = &rows[row_idx];
            if row.nonselectable || row.disabled {
                return;
            }
            let id = row.id.clone().unwrap_or_default();
            let text = row.text.clone();

            let mut selected_ids = Vec::new();
            let mut selected_texts = Vec::new();

            if toggled_indices.is_empty() {
                selected_ids.push(id.clone());
                selected_texts.push(text.clone());
            } else {
                // Return all toggled rows in order they appear in the *original* rows vector
                // (or filtered vector? Usually original rows order).
                for (idx, r) in rows.iter().enumerate() {
                    if toggled_indices.contains(&idx) {
                        selected_ids.push(r.id.clone().unwrap_or_default());
                        selected_texts.push(r.text.clone());
                    }
                }
            }

            if is_action {
                crate::core::gui_protocol::GuiEvent::Action {
                    key: key.to_string(),
                    index: row_idx,
                    id,
                    text,
                    retv,
                    data: data.clone(),
                    selected_ids,
                    selected_texts,
                }
            } else {
                crate::core::gui_protocol::GuiEvent::Select {
                    key: key.to_string(),
                    index: row_idx,
                    id,
                    text,
                    retv,
                    data: data.clone(),
                    selected_ids,
                    selected_texts,
                }
            }
        } else if !no_custom && !query.is_empty() {
            crate::core::gui_protocol::GuiEvent::Custom {
                key: key.to_string(),
                text: query.clone(),
                retv: 2,
                data: data.clone(),
            }
        } else {
            return;
        };

        let Some(session) = self.gui_session.clone() else {
            self.gui_leave();
            return;
        };

        // Mark loading state while waiting for the next response
        if let LauncherState::GuiMode { loading, .. } = &mut self.state {
            *loading = true;
        }

        let view = cx.entity();
        let cx_async = cx.to_async();
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = std::thread::Builder::new()
            .name("aerofi-gui-event".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                let result = {
                    let mut session = session.lock().unwrap();
                    if session.send_event(&event).is_err() {
                        ReadResult::Exited
                    } else {
                        session.read_burst(std::time::Duration::from_secs(5))
                    }
                };
                let _ = tx.send(result);
            });
        cx.spawn(move |_, _: &mut gpui::AsyncApp| async move {
            if let Ok(result) = rx.await {
                cx_async.update(|cx| {
                    view.update(cx, |launcher, cx| {
                        launcher.gui_handle_read_result(result, &title);
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    /// Send a live search query update to the script's stdin.
    fn gui_send_live_search(&mut self, cx: &mut Context<Self>) {
        let LauncherState::GuiMode {
            query,
            title,
            live_search,
            ..
        } = &self.state
        else {
            return;
        };

        if !*live_search {
            return;
        }

        let query = query.clone();
        let title = title.clone();
        let Some(session) = self.gui_session.clone() else {
            return;
        };

        // Mark loading while script recalculates
        if let LauncherState::GuiMode { loading, .. } = &mut self.state {
            *loading = true;
        }

        let view = cx.entity();
        let cx_async = cx.to_async();
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = std::thread::Builder::new()
            .name("aerofi-gui-search".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                let result = {
                    let mut session = session.lock().unwrap();
                    if session.send_query_change(&query).is_err() {
                        ReadResult::Exited
                    } else {
                        session.read_burst(std::time::Duration::from_secs(5))
                    }
                };
                let _ = tx.send(result);
            });
        cx.spawn(move |_, _: &mut gpui::AsyncApp| async move {
            if let Ok(result) = rx.await {
                cx_async.update(|cx| {
                    view.update(cx, |launcher, cx| {
                        launcher.gui_handle_read_result(result, &title);
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    /// Re-filter GUI rows based on the current query.
    fn gui_refilter(&mut self) {
        if let LauncherState::GuiMode {
            rows,
            filtered_rows,
            query,
            selected,
            ..
        } = &mut self.state
        {
            *filtered_rows = gui_session::filter_gui_rows(rows, query);
            if *selected >= filtered_rows.len() {
                *selected = 0;
            }
        }
    }

    /// Leave GUI mode: kill the session and return to the search list.
    fn gui_leave(&mut self) {
        if let Some(session) = self.gui_session.take()
            && let Ok(mut s) = session.lock()
        {
            s.kill();
        }
        // Clear sticky_metatags so the GUI script's layout overrides (e.g.
        // `@aerofi.columns 1`) don't linger after the script exits.
        // Without this, a 4-column grid theme would render as a 1-column list
        // after closing a gui-mode script that declared columns = 1.
        self.sticky_metatags = None;
        let new_config = crate::core::config::AppConfig::load();
        if new_config.theme != self.app_config.theme {
            self.reload();
        }
        self.state = LauncherState::Search;
    }
}
