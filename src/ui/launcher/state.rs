//! Launcher state: struct definition and controller logic (keystroke
//! handling, filtering, execution, configuration reload).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Context, Font, ListAlignment, ListState, ScrollStrategy, UniformListScrollHandle, px};

use crate::core::config::AppConfig;
use crate::core::gui_protocol::GuiCommand;
use crate::core::gui_session::{self, GuiSession, ReadResult};
use crate::core::history::History;
use crate::core::item::{BuiltinAction, ScriptMetatags, ScriptMode, Target};
use crate::core::search::SearchIndex;
use crate::core::theme::ThemeConfig;
use crate::core::widget::WidgetRegistry;

use super::helpers::combo_matches;
use super::types::{LauncherAction, LauncherState, MdStyled};

/// Pango parse result for a GUI row: the plain text plus the highlight
/// style for each markup span.
type PangoParse = (String, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>);

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
    /// Active theme controlling every visual aspect of the launcher. Shared
    /// via `Arc` so spawning a script execution only bumps a refcount
    /// instead of deep-cloning the whole config tree.
    pub(super) theme: Arc<ThemeConfig>,
    /// Current state of the launcher (e.g. normal search or showing script output).
    pub(super) state: LauncherState,
    /// List state for fullOutput mode (variable-height markdown blocks).
    pub(super) full_output_list: ListState,
    /// List state for the GUI-mode markdown preview panel (variable-height
    /// blocks, kept separate so its scroll position is independent).
    pub(super) preview_list: ListState,
    /// Scroll state for the GUI-mode rows list (kept separate from the
    /// search list so each mode's scroll position is independent).
    pub(super) gui_rows_scroll: UniformListScrollHandle,
    /// Parsed markdown blocks of the current full-output result (empty
    /// while not showing one; cleared on hide to release the memory).
    pub(super) full_output_blocks: Vec<crate::core::markdown::MdBlock>,
    /// Precomputed markdown styling, parallel to `full_output_blocks`
    /// (None for blocks without inline text). Rebuilt whenever the blocks
    /// change and when the theme is reloaded.
    pub(super) full_output_styled: Vec<Option<MdStyled>>,
    /// Layout overrides (`@aerofi.show_search` / `@aerofi.columns`)
    /// committed by the last executed script. Kept until the launcher is
    /// hidden; never applied by selection alone.
    pub(super) sticky_metatags: Option<ScriptMetatags>,
    /// Registry of custom widget definitions from `[[widgets]]` in the
    /// theme file. Used by `render_custom_widget` in `custom_widgets.rs`.
    pub(super) widget_registry: WidgetRegistry,
    /// Hotkey bindings from button widgets: maps combo string (e.g. "cmd+r")
    /// to the button's action string. Rebuilt on config reload.
    pub(super) button_hotkeys: HashMap<String, (String, bool)>,
    /// Active GUI-mode script session (stdin/stdout pipe to child).
    /// Wrapped in Arc<Mutex<>> so it can be shared with async tasks.
    pub(super) gui_session: Option<Arc<Mutex<GuiSession>>>,
    /// Single pump thread serving live-search queries for the active GUI
    /// session. Kept alive while the session runs; dropped on leave/hide,
    /// which terminates the thread.
    pub(super) gui_live_search: Option<LiveSearchPump>,
    /// Manager for dynamic `.dylib` plugins.
    pub(super) plugin_manager: crate::core::plugin_manager::PluginManager,
    /// Active plugin search task.
    pub(super) plugin_search_task: Option<gpui::Task<()>>,
    /// Pending background icon-extraction task (startup), if any.
    pub(super) icon_task: Option<gpui::Task<()>>,
    /// Number of base items (apps/scripts/builtins). Plugin items are appended after this.
    pub(super) base_count: usize,
    /// Set to `true` after a config/theme reload so the next `render()` call
    /// re-centers the window *after* `window.resize()` has been applied.
    pub(super) needs_center: bool,
    /// Last (width, height) in points passed to `window.resize()`. GPUI's
    /// macOS `resize` unconditionally calls `setContentSize_`, so we only
    /// forward a resize (and the re-center it triggers) when the size
    /// actually changed — otherwise every frame moves the native window.
    pub(super) last_window_size: Option<(f32, f32)>,
    /// Last `[window] blur` value applied via `window.set_background_appearance`.
    /// GPUI sets the background once at window creation and its blur view
    /// (NSVisualEffectView) persists across hide/unhide and theme changes, so
    /// `render()` re-syncs it whenever the active theme's blur differs.
    pub(super) last_blur: Option<bool>,
    /// Pre-computed font fallback families for the theme. Avoids rebuilding
    /// the Vec on every element per frame.
    pub(super) font_fallbacks: Vec<String>,
    /// Pre-computed GPUI font (family + fallbacks + weight) for the root
    /// div. Cloning it per frame is cheap (Arc bumps) and avoids rebuilding
    /// the fallback cascade + family strings on every frame.
    pub(super) root_font: gpui::Font,
    /// Pre-computed (font, size) for the input bar.
    pub(super) inputbar_font: (gpui::Font, f32),
    /// Pre-computed (font, size) for list rows/cells (the `[element].font`
    /// override). Reused by every visible row/cell each frame.
    pub(super) element_font_val: (gpui::Font, f32),
    /// Pre-computed monospace family for code blocks / table cells (first
    /// fallback containing "mono", else "SF Mono"). Derived from the theme,
    /// recomputed on reload.
    pub(super) mono_font: gpui::SharedString,
    /// Query match ranges (byte offsets) memoized per rendered text. Cleared
    /// on every `refilter()` (i.e. on any query change), so selection
    /// changes and plain redraws don't re-run the fuzzy matcher for each
    /// visible row on every render pass.
    pub(super) highlight_cache: std::cell::RefCell<
        std::collections::HashMap<gpui::SharedString, Vec<std::ops::Range<usize>>>,
    >,
    /// Pango markup parse results memoized per GUI row text. Cleared
    /// whenever a new burst installs new rows, since the parse output
    /// depends only on the row text.
    pub(super) pango_cache:
        std::cell::RefCell<std::collections::HashMap<gpui::SharedString, PangoParse>>,
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
        let font_fallbacks = super::helpers::font_fallback_families(&theme.font.fallback);
        let (root_font, inputbar_font, element_font_val, mono_font) =
            Self::build_fonts(&theme, &font_fallbacks);
        Self {
            all,
            filtered,
            query: String::new(),
            selected: 0,
            list: UniformListScrollHandle::new(),
            search: SearchIndex::new(&app_config.aliases),
            history,
            app_config,
            theme: Arc::new(theme),
            state: LauncherState::Search,
            full_output_list: ListState::new(0, ListAlignment::Top, px(16.0)),
            preview_list: ListState::new(0, ListAlignment::Top, px(16.0)),
            gui_rows_scroll: UniformListScrollHandle::new(),
            full_output_blocks: Vec::new(),
            full_output_styled: Vec::new(),
            gui_live_search: None,
            sticky_metatags: None,
            widget_registry,
            button_hotkeys,
            gui_session: None,
            plugin_manager: crate::core::plugin_manager::PluginManager::load_all(),
            plugin_search_task: None,
            icon_task: None,
            base_count,
            needs_center: false,
            last_window_size: None,
            last_blur: None,
            font_fallbacks,
            root_font,
            inputbar_font,
            element_font_val,
            mono_font,
            highlight_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
            pango_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// Build the cached GPUI fonts (root, input bar, list element) from the
    /// theme. Called once per theme load so per-frame rendering only clones
    /// the (cheap) `Font` values instead of rebuilding fallback cascades.
    fn build_fonts(
        theme: &ThemeConfig,
        font_fallbacks: &[String],
    ) -> (Font, (Font, f32), (Font, f32), gpui::SharedString) {
        let base_weight = super::helpers::base_weight(&theme.font.weight);
        let root_font = Font {
            family: theme.font.family.clone().into(),
            features: gpui::FontFeatures::default(),
            fallbacks: Some(gpui::FontFallbacks::from_fonts(font_fallbacks.to_vec())),
            weight: base_weight,
            style: gpui::FontStyle::Normal,
        };
        let inputbar_font = super::helpers::element_font(
            &theme.font,
            theme.inputbar.font.as_ref(),
            base_weight,
            font_fallbacks,
        );
        let element_font_val = super::helpers::element_font(
            &theme.font,
            theme.element.font.as_ref(),
            base_weight,
            font_fallbacks,
        );
        let mono_font = super::helpers::mono_family(theme);
        (root_font, inputbar_font, element_font_val, mono_font)
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
            let action = self.execute_item(&item, false);
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
            && let Some((_, (action, stay_open))) = self
                .button_hotkeys
                .iter()
                .find(|(combo, _)| combo_matches(combo, ks))
        {
            let (action, stay_open) = (action.clone(), *stay_open);
            if let Some(cx) = cx {
                self.handle_widget_button_action(&action, None, stay_open, cx);
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
                // Plain Enter activates a running app; Shift+Enter opens a new
                // instance (`open -n`).
                let action = self.execute_selected(ks.modifiers.shift);
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
                        let action = self.execute_item(&item, false);
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
        // A new query changes every row's highlight ranges; drop the memo.
        self.highlight_cache.borrow_mut().clear();
        // Clear any previous plugin items and release excess capacity.
        self.all.truncate(self.base_count);
        if self.all.capacity() > self.base_count * 2 {
            self.all.shrink_to(self.base_count);
        }

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
                self.filtered.truncate(self.app_config.general.max_results);

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
                                    let mut parsed = plugin_clone.parse_results(&results);
                                    plugin_clone.free_results(results);
                                    // Swap full-resolution image paths for
                                    // bounded thumbnails before the rows hit
                                    // the renderer (first use decodes here,
                                    // off the main thread).
                                    crate::sys::icons::thumbnail_image_icons(&mut parsed);
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
                let parsed = plugin.parse_results(&results);
                plugin.free_results(results);
                self.all.extend(parsed);

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
            self.filtered.truncate(self.app_config.general.max_results);
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
        // A still-running script can't be resumed after hide, so revert
        // to search.  A finished full-output view preserves its blocks so
        // the user can come back to them.
        if matches!(self.state, LauncherState::RunningFull { .. }) {
            self.state = LauncherState::Search;
            self.full_output_blocks.clear();
            self.full_output_styled.clear();
            self.full_output_list.reset(0);
        }
        // Kill any active GUI session.
        if let Some(session) = self.gui_session.take()
            && let Ok(mut s) = session.lock()
        {
            s.kill();
        }
        // Stop the live-search pump thread (dropping the sender makes its
        // recv() return and the thread exits).
        self.gui_live_search.take();
        // Best-effort: ask the allocator to munmap the session's freed
        // pages while we are hidden. Modern macOS often releases nothing
        // (see sys::memory); harmless either way.
        crate::sys::memory::pressure_relief();
        // Tell the GPU driver to evict the atlases/drawables we no longer
        // render; they re-fault on the next show.
        crate::sys::gpu::trim_gpu_memory();
    }

    /// Called when the window is shown.  Refills `filtered` from `all`
    /// so the next render creates fresh `img()` elements that GPUI will
    /// decode on demand.
    pub fn on_show(&mut self) {
        // Lift the hidden GPU working-set budget before the first frame so
        // the driver keeps the re-faulted buffers resident.
        crate::sys::gpu::restore_gpu_memory();
        // Inline daemons buffered their ticks while the window was hidden
        // (the main thread was never woken for them); apply the latest of
        // each before the first frame so subtitles aren't stale.
        for (path, text) in crate::core::scheduler::flush_pending_inline() {
            self.apply_inline_output(&path, Some(gpui::SharedString::from(text)));
        }
        self.refilter(None);
        // refilter() resets the scroll to the top, but the selection
        // survived the hide/show cycle — restore the view to it so the
        // user doesn't lose their place. In grid mode the list items are
        // rows, so convert the flat selection index to a grid-row index.
        let cols = self.effective_columns().max(1);
        let row_ix = if cols > 1 {
            self.selected / cols
        } else {
            self.selected
        };
        self.list.scroll_to_item(row_ix, ScrollStrategy::Nearest);
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

    /// Run the highlighted target. `new_instance` (Shift+Enter on an app)
    /// launches a fresh instance; plain Enter activates the running one.
    pub(super) fn execute_selected(&mut self, new_instance: bool) -> LauncherAction {
        let Some(item) = self.selected_item() else {
            return LauncherAction::None;
        };
        let item = item.clone();
        self.execute_item(&item, new_instance)
    }

    /// Run a target: built-in actions act in place, apps open and scripts
    /// run asynchronously via `LauncherAction::ExecuteScript`.
    pub(super) fn execute_item(&mut self, item: &Target, new_instance: bool) -> LauncherAction {
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
            Target::App { path, .. } => {
                let identifier = item.identifier();
                crate::core::executor::open_app(path, item.name(), new_instance);
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
        let blocks = crate::core::markdown::parse(&text);
        self.full_output_styled = blocks.iter().map(|b| self.build_md_styled(b)).collect();
        self.full_output_blocks = blocks;
        // Reset to the top with the new item count.
        self.full_output_list.reset(self.full_output_blocks.len());
        self.state = LauncherState::FullOutput { title };
    }

    /// Run the side effects of a `LauncherAction` produced inside the view.
    ///
    /// Both input paths funnel through here: the keystroke observer in
    /// `main.rs` and the mouse listeners on the launcher's elements. Async
    /// work is spawned, so this returns immediately.
    pub fn perform_action(
        &mut self,
        action: LauncherAction,
        stay_open: bool,
        cx: &mut Context<Self>,
    ) {
        match action {
            LauncherAction::None => {}
            LauncherAction::Hide => {
                if !stay_open {
                    self.on_hide();
                    cx.notify();
                    crate::ui::window::hide();
                }
            }
            LauncherAction::CopyToClipboardAndHide(text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                if !stay_open {
                    self.on_hide();
                    cx.notify();
                    crate::ui::window::hide();
                }
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
                if !stay_open
                    && let Target::Script { mode, .. } = &target
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
                    && !stay_open
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
            self.full_output_styled.clear();
            self.full_output_list.reset(0);
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
        // Reconcile inline-script daemons with the new target set (scripts
        // added/removed/re-intervalled by the reload).
        crate::core::scheduler::reconcile_daemons(&self.all);
        self.search = SearchIndex::new(&self.app_config.aliases);
        self.plugin_manager = crate::core::plugin_manager::PluginManager::load_all();
        self.widget_registry = WidgetRegistry::from_theme(&theme.widgets);
        self.button_hotkeys = self.widget_registry.button_hotkeys();
        self.theme = Arc::new(theme);
        self.font_fallbacks = super::helpers::font_fallback_families(&self.theme.font.fallback);
        let (root_font, inputbar_font, element_font_val, mono_font) =
            Self::build_fonts(self.theme.as_ref(), &self.font_fallbacks);
        self.root_font = root_font;
        self.inputbar_font = inputbar_font;
        self.element_font_val = element_font_val;
        self.mono_font = mono_font;
        // Markdown styling is theme-derived (link color, code background,
        // mono family), so rebuild it for any blocks still on screen.
        self.full_output_styled = self
            .full_output_blocks
            .iter()
            .map(|b| self.build_md_styled(b))
            .collect();
        if let LauncherState::GuiMode {
            preview_blocks: Some(blocks),
            ..
        } = &self.state
        {
            let styled: Vec<Option<MdStyled>> =
                blocks.iter().map(|b| self.build_md_styled(b)).collect();
            if let LauncherState::GuiMode { preview_styled, .. } = &mut self.state {
                *preview_styled = Some(styled);
            }
        }
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

    /// Start background icon processing for the given jobs (collected on
    /// the main thread by [`crate::sys::icons::prepare_icon_jobs`]). The
    /// CPU-heavy downsample runs on a worker thread; results are applied to
    /// `all` when they arrive, so the first frame never waits on it.
    pub fn start_icon_jobs(
        &mut self,
        jobs: Vec<crate::sys::icons::IconJob>,
        cx: &mut Context<Self>,
    ) {
        if jobs.is_empty() {
            return;
        }
        let (tx, rx) = futures::channel::oneshot::channel();
        if let Err(e) = std::thread::Builder::new()
            .name("aerofi-icon-extract".into())
            .spawn(move || {
                let _ = tx.send(crate::sys::icons::process_icon_jobs(jobs));
            })
        {
            eprintln!("aerofi: failed to spawn icon worker thread: {e}");
            return;
        }
        self.icon_task = Some(
            cx.spawn(|view: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                // Clone into an owned handle so the future is 'static.
                let mut cx = cx.clone();
                async move {
                    let Ok(results) = rx.await else {
                        return;
                    };
                    let _ = view.update(&mut cx, |this, cx| {
                        crate::sys::icons::apply_icon_results(&mut this.all, results);
                        cx.notify();
                    });
                }
            }),
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
        stay_open: bool,
        cx: &mut Context<Self>,
    ) {
        let trimmed = action.trim();

        // Row-context actions
        if trimmed.eq_ignore_ascii_case("run") || trimmed.eq_ignore_ascii_case("execute") {
            if let Some(item) = row_item.cloned() {
                let action = self.execute_item(&item, false);
                self.perform_action(action, stay_open, cx);
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
            let action = self.execute_item(&target, false);
            self.perform_action(action, stay_open, cx);
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
        let layout = target.metatags().and_then(|m| m.layout.clone());

        // Metatags are committed only when we actually enter GUI mode (see
        // `show_gui_loading` / the burst task below) — not here — so the
        // search list doesn't render with the GUI layout (columns, hidden
        // input bar) during the brief wait for the first burst.
        let metatags = target.metatags().cloned();

        // Record in history.
        let identifier = target.identifier();
        self.history.record_launch(identifier);

        let path = path.clone();
        let mut envs = std::collections::HashMap::new();
        envs.insert("AEROFI_RETV".to_string(), "0".to_string());

        match GuiSession::spawn(&path, args, envs) {
            Ok(session) => {
                let session = Arc::new(Mutex::new(session));
                // A fresh session gets a fresh live-search pump (the old one
                // is bound to the previous session's pipes).
                self.gui_live_search.take();
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
                let metatags_burst = metatags.clone();
                cx.spawn(move |_, _: &mut gpui::AsyncApp| async move {
                    if let Ok(burst) = rx.await {
                        cx_async.update(|cx| {
                            view.update(cx, |launcher, cx| {
                                // Commit the layout metatags as we enter GUI
                                // mode. If the script exited without output,
                                // `gui_leave()` below clears them again.
                                launcher.sticky_metatags = metatags_burst;
                                launcher.gui_handle_read_result(burst, &title2);
                                cx.notify();
                            });
                        });
                    }
                })
                .detach();

                // Show a "loading" frame only if the script is slow to emit
                // its first burst. Fast scripts go straight from the search
                // list to the populated GUI, so the user never sees an empty
                // flash. The task no-ops if a burst (or an exit/hide) already
                // landed before the delay elapsed.
                let title_loading = title.clone();
                let layout_loading = layout.clone();
                let metatags_loading = metatags;
                cx.spawn(
                    move |view: gpui::WeakEntity<Self>, app: &mut gpui::AsyncApp| {
                        let mut app = app.clone();
                        async move {
                            app.background_executor()
                                .timer(std::time::Duration::from_millis(120))
                                .await;
                            let _ = view
                                .update(&mut app, |launcher, cx| {
                                    launcher.show_gui_loading(
                                        &title_loading,
                                        layout_loading,
                                        metatags_loading,
                                        cx,
                                    );
                                })
                                .ok();
                        }
                    },
                )
                .detach();
            }
            Err(e) => {
                eprintln!("aerofi: failed to start GUI session for {title}: {e}");
            }
        }
    }

    /// Show the temporary "loading" GUI frame, but only if a burst hasn't
    /// landed yet (we're still not in `GuiMode`) and the session is still
    /// alive. Invoked by a delayed task from [`start_gui_session`], so fast
    /// scripts never trigger it — the user goes straight from the search list
    /// to the populated GUI with no empty flash.
    fn show_gui_loading(
        &mut self,
        title: &str,
        layout: Option<String>,
        metatags: Option<ScriptMetatags>,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.state, LauncherState::GuiMode { .. }) || self.gui_session.is_none() {
            return;
        }
        // A new GUI session starts with fresh rows.
        self.pango_cache.borrow_mut().clear();
        // Commit the layout metatags as we enter GUI mode (loading frame).
        self.sticky_metatags = metatags;
        self.state = LauncherState::GuiMode {
            title: title.to_string(),
            rows: Arc::new(Vec::new()),
            filtered_rows: Vec::new(),
            prompt: None,
            message: Some("Loading…".to_string()),
            no_custom: false,
            selected: 0,
            // Match the committed layout so the window is already at its final
            // size when the burst lands (avoids a resize right after loading).
            columns: self.sticky_metatags.as_ref().and_then(|m| m.columns),
            query: String::new(),
            loading: true,
            live_search: false,
            active_indices: Vec::new(),
            data: None,
            preview_blocks: None,
            preview_styled: None,
            multi_select: false,
            toggled_indices: std::collections::HashSet::new(),
            markup_rows: false,
            layout,
        };
        cx.notify();
    }

    /// Handle the read result from a GUI session burst.
    fn gui_handle_read_result(&mut self, result: ReadResult, title: &str) {
        match result {
            ReadResult::Burst(burst) => {
                self.gui_apply_burst(burst, title);
            }
            ReadResult::BurstThenExit(burst) => {
                self.gui_apply_burst(burst, title);
                // Script exited after this burst — mark session as done and
                // drop its live-search pump (a new session must not reuse it).
                // The UI will stay showing the last rows but selecting will
                // leave GUI mode.
                self.gui_session = None;
                self.gui_live_search.take();
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
    pub(super) fn gui_apply_burst(
        &mut self,
        burst: crate::core::gui_protocol::GuiBurst,
        title: &str,
    ) {
        // The burst installs new rows; drop memoized parses of old texts.
        self.pango_cache.borrow_mut().clear();
        let mut prompt = None;
        let mut message = None;
        let mut no_custom = false;
        let mut keep_selection: Option<String> = None;
        let mut columns = self.sticky_metatags.as_ref().and_then(|m| m.columns);
        let mut loading = false;
        let mut live_search = false;
        let mut active_indices = Vec::new();
        let mut data = None;
        let mut preview_blocks = None;
        let mut preview_styled = None;
        let mut preview_changed = false;
        let mut multi_select = false;
        let mut toggled_indices = std::collections::HashSet::new();
        let mut markup_rows = false;
        let mut layout: Option<String> = None;

        // Inherit flags if we are updating an existing GuiMode session
        if let LauncherState::GuiMode {
            prompt: prev_prompt,
            no_custom: prev_no_custom,
            columns: prev_columns,
            live_search: prev_live_search,
            active_indices: prev_active_indices,
            data: prev_data,
            preview_blocks: prev_preview_blocks,
            preview_styled: prev_preview_styled,
            multi_select: prev_multi_select,
            toggled_indices: prev_toggled_indices,
            markup_rows: prev_markup_rows,
            layout: prev_layout,
            ..
        } = &self.state
        {
            prompt = prev_prompt.clone();
            no_custom = *prev_no_custom;
            columns = columns.or(*prev_columns);
            live_search = *prev_live_search;
            active_indices = prev_active_indices.clone();
            data = prev_data.clone();
            preview_blocks = prev_preview_blocks.clone();
            preview_styled = prev_preview_styled.clone();
            multi_select = *prev_multi_select;
            toggled_indices = prev_toggled_indices.clone();
            markup_rows = *prev_markup_rows;
            layout = prev_layout.clone();
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
                    let blocks = crate::core::markdown::parse(text);
                    preview_styled = Some(blocks.iter().map(|b| self.build_md_styled(b)).collect());
                    preview_blocks = Some(blocks);
                    preview_changed = true;
                }
                GuiCommand::PreviewFile(file_path) => {
                    let resolved = super::helpers::expand_tilde_path(file_path);
                    if let Ok(text) = std::fs::read_to_string(&resolved) {
                        let blocks = crate::core::markdown::parse(&text);
                        preview_styled =
                            Some(blocks.iter().map(|b| self.build_md_styled(b)).collect());
                        preview_blocks = Some(blocks);
                        preview_changed = true;
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
                    // gui_leave() already reloaded when the theme changed on
                    // disk; only reload again for other config changes so a
                    // theme switch triggers exactly one full re-index.
                    if !self.gui_leave() {
                        self.reload();
                    }
                    return;
                }
            }
        }

        if preview_changed {
            self.preview_list
                .reset(preview_blocks.as_ref().map_or(0, |b| b.len()));
        }

        let row_count = burst.rows.len();
        let filtered_rows: Vec<usize> = (0..row_count).collect();

        // Find pre-selected row if requested.
        let selected = keep_selection
            .and_then(|sel| burst.rows.iter().position(|r| r.text == sel))
            .unwrap_or(0);

        self.state = LauncherState::GuiMode {
            title: title.to_string(),
            rows: Arc::new(burst.rows),
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
            preview_styled,
            multi_select,
            toggled_indices,
            markup_rows,
            layout,
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
    /// Enqueue a live-search query on the session's single pump thread.
    ///
    /// Previously every keystroke spawned a thread that held the session
    /// mutex for a full `read_burst` (up to 5 s); fast typing piled threads
    /// up on the lock and results arrived tens of seconds late. Now one
    /// pump thread per session coalesces keystrokes (only the newest query
    /// is processed), sends it to the script, and drains output until the
    /// script goes quiet.
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

        // Start the pump lazily on the first live-search keystroke.
        let pump = match self.gui_live_search.as_mut() {
            Some(p) => p,
            None => {
                let pump = Self::start_live_search_pump(session, title.clone(), cx.entity(), cx);
                self.gui_live_search = Some(pump);
                self.gui_live_search.as_mut().expect("pump just started")
            }
        };
        let query_generation = pump.latest_enqueued.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = pump.tx.send((query, query_generation));
    }

    /// Spawn the live-search pump thread for `session` and return its
    /// control handle. The thread exits when the returned `LiveSearchPump`
    /// is dropped (its sender half).
    fn start_live_search_pump(
        session: Arc<Mutex<GuiSession>>,
        title: String,
        view: gpui::Entity<Launcher>,
        cx: &mut Context<Self>,
    ) -> LiveSearchPump {
        let latest_enqueued = Arc::new(AtomicU64::new(0));
        let (query_tx, query_rx) = mpsc::channel::<(String, u64)>();
        // The pump thread is a plain OS thread (it blocks on pipes), so it
        // can't hold GPUI handles; finished results are handed to a
        // GPUI-side task over this channel. Dropping `result_tx` (when the
        // pump thread exits) ends the task.
        let (mut result_tx, mut result_rx) = futures::channel::mpsc::channel::<LiveSearchApply>(16);

        let pump_latest = latest_enqueued.clone();
        let _ = std::thread::Builder::new()
            .name("aerofi-gui-live-search".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                while let Ok((mut query, mut query_generation)) = query_rx.recv() {
                    // Coalesce: if the user typed several keys while we
                    // were busy, only the newest (query, generation) matters.
                    while let Ok((later_query, later_generation)) = query_rx.try_recv() {
                        query = later_query;
                        query_generation = later_generation;
                    }

                    let result = Self::send_and_drain(&session, &query);
                    let stale = pump_latest.load(Ordering::Relaxed) != query_generation;
                    let _ = result_tx.try_send(LiveSearchApply {
                        result,
                        title: title.clone(),
                        session: session.clone(),
                        stale,
                    });
                }
                // Drop the channel end: the GPUI-side task sees EOF and ends.
            });

        let cx_async = cx.to_async();
        cx.spawn(move |_, _: &mut gpui::AsyncApp| async move {
            while let Ok(apply) = result_rx.recv().await {
                let LiveSearchApply {
                    result,
                    title,
                    session,
                    stale,
                } = apply;
                cx_async.update(|cx| {
                    view.update(cx, |launcher, cx| {
                        // Ignore results from a session that is no longer
                        // active (left GUI mode, or a new session started)
                        // and skip frames superseded by a newer query.
                        let active = launcher
                            .gui_session
                            .as_ref()
                            .is_some_and(|s| Arc::ptr_eq(s, &session));
                        if active && !stale {
                            if let Some(result) = result {
                                launcher.gui_handle_read_result(result, &title);
                            }
                            if let LauncherState::GuiMode { loading, .. } = &mut launcher.state {
                                *loading = false;
                            }
                            cx.notify();
                        }
                    });
                });
            }
        })
        .detach();

        LiveSearchPump {
            latest_enqueued,
            tx: query_tx,
        }
    }

    /// Send a live-search query to the script, then drain its output until
    /// the script goes quiet. Returns `None` if the script produced nothing
    /// within the first-line deadline (the UI stays on its current rows).
    fn send_and_drain(session: &Mutex<GuiSession>, query: &str) -> Option<ReadResult> {
        // Lock is held only for the stdin write itself.
        {
            let mut s = session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if s.send_query_change(query).is_err() {
                return Some(ReadResult::Exited);
            }
        }

        // Drain in 100 ms chunks: each `read_burst` already swallows a
        // complete burst (50 ms inter-line quiet), so normally one chunk
        // suffices. `try_lock` keeps UI operations (row selection, events)
        // from ever blocking on our read. The last non-empty burst is the
        // script's final frame for this query.
        let first_line_deadline = Instant::now() + Duration::from_secs(2);
        let mut last: Option<ReadResult> = None;
        loop {
            let chunk = match session.try_lock() {
                Ok(s) => s.read_burst(Duration::from_millis(100)),
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            let quiet = matches!(
                &chunk,
                ReadResult::Burst(b) if b.rows.is_empty() && b.commands.is_empty()
            );
            if quiet {
                if last.is_some() || Instant::now() >= first_line_deadline {
                    break;
                }
                continue; // still waiting for the script's first line
            }
            if matches!(
                chunk,
                ReadResult::Exited | ReadResult::Error(_) | ReadResult::BurstThenExit(_)
            ) {
                return Some(chunk);
            }
            last = Some(chunk);
        }
        last
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
    /// Leave GUI mode. Returns `true` if a reload was performed because the
    /// theme changed on disk (callers that also want a reload — e.g. the
    /// `Reload` GUI command — can skip their own to avoid a double re-index).
    fn gui_leave(&mut self) -> bool {
        if let Some(session) = self.gui_session.take()
            && let Ok(mut s) = session.lock()
        {
            s.kill();
        }
        // Stop the live-search pump thread for this session.
        self.gui_live_search.take();
        // Clear sticky_metatags so the GUI script's layout overrides (e.g.
        // `@aerofi.columns 1`) don't linger after the script exits.
        // Without this, a 4-column grid theme would render as a 1-column list
        // after closing a gui-mode script that declared columns = 1.
        self.sticky_metatags = None;
        let new_config = crate::core::config::AppConfig::load();
        let reloaded = new_config.theme != self.app_config.theme;
        if reloaded {
            self.reload();
        }
        self.state = LauncherState::Search;

        // Pre-size the window synchronously to the Search layout dimensions.
        //
        // `render()` resizes the window via an async `window.resize()` call.
        // Between the state switch and the async resize landing, GPUI draws
        // the new Search content at the old (small) GUI-mode window size.
        // CoreAnimation then stretches that frame to the new (larger) window
        // size until the next draw — producing a visible "stretched image"
        // artifact most noticeable when a theme has a full-height artwork
        // image on the left pane (e.g. graphite-mono).
        //
        // Calling `setContentSize:` here (in the keystroke/event handler,
        // outside any draw pass) applies the resize synchronously so the
        // very first Search frame is drawn at the correct size.  The later
        // async `window.resize()` from `render()` becomes a harmless no-op
        // (same size → early return in `set_frame_size`).
        let t = &self.theme;
        crate::sys::appkit::set_window_size(t.window.width as f64, t.window.height as f64);
        reloaded
    }
}

/// Handle for the single live-search pump thread of an active GUI session.
///
/// Dropping the value (on `gui_leave` / `on_hide`) drops the sender half,
/// which makes the pump thread's `recv()` return and the thread exit.
pub(super) struct LiveSearchPump {
    /// Generation of the newest enqueued query. The pump skips applying a
    /// result once a newer query has been enqueued (the next loop
    /// iteration, coalesced to the latest, renders the fresh result).
    latest_enqueued: Arc<AtomicU64>,
    /// Sender half: each live-search keystroke pushes (query, generation).
    tx: mpsc::Sender<(String, u64)>,
}

/// One finished live-search round, handed from the pump thread to the
/// GPUI-side task that applies it on the main thread.
struct LiveSearchApply {
    /// `None` if the script produced no output within the deadline.
    result: Option<ReadResult>,
    title: String,
    /// The session this result belongs to; stale if no longer the active one.
    session: Arc<Mutex<GuiSession>>,
    /// `true` if a newer query was enqueued while this round was draining.
    stale: bool,
}
