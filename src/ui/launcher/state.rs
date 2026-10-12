//! Launcher state: struct definition and controller logic (keystroke
//! handling, filtering, execution, configuration reload).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gpui::{Context, Font, ListAlignment, ListState, ScrollStrategy, UniformListScrollHandle, px};

use crate::core::config::{AppConfig, NewInstanceMode};
use crate::core::history::History;
use crate::core::item::{BuiltinAction, ScriptMetatags, ScriptMode, SourceMask, Target};
use crate::core::rofi_session::RofiSession;
use crate::core::search::SearchIndex;
use crate::core::theme::ThemeConfig;
use crate::core::widget::WidgetRegistry;

use super::sessions::{LiveSearchPump, PluginSearchPump, PluginSearchRequest};
use super::types::{LauncherAction, LauncherState, MdStyled};

/// Pango parse result for a Rofi row: the plain text plus the highlight
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
    /// List state for the Rofi-mode markdown preview panel (variable-height
    /// blocks, kept separate so its scroll position is independent).
    pub(super) preview_list: ListState,
    /// Scroll state for the Rofi-mode rows list (kept separate from the
    /// search list so each mode's scroll position is independent).
    pub(super) rofi_rows_scroll: UniformListScrollHandle,
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
    /// Natural dimensions (width, height) of the images referenced by the
    /// theme's widgets, pre-resolved at theme load so `auto_height` can
    /// measure width-constrained images without touching the disk per frame.
    pub(super) image_dims: std::collections::HashMap<String, (u32, u32)>,
    /// The search list's viewport height for the current frame, computed
    /// alongside the window height (see `render`). The scrollbar overlay
    /// in `render_listview` needs it as the track size. Zero when the
    /// list is not rendered this frame (non-search states).
    pub(super) last_list_viewport_h: f32,
    /// Hotkey bindings from button widgets: maps combo string (e.g. "cmd+r")
    /// to the button's action string. Rebuilt on config reload.
    pub(super) button_hotkeys: HashMap<String, (String, bool)>,
    /// Active Rofi-mode script session (stdin/stdout pipe to child).
    /// Wrapped in Arc<Mutex<>> so it can be shared with async tasks.
    pub(super) rofi_session: Option<Arc<Mutex<RofiSession>>>,
    /// Single pump thread serving live-search queries for the active Rofi
    /// session. Kept alive while the session runs; dropped on leave/hide,
    /// which terminates the thread.
    pub(super) rofi_live_search: Option<LiveSearchPump>,
    /// Whether the active Rofi session was launched from a hidden window (a
    /// global hotkey) rather than from the visible search list. When true,
    /// exiting the Rofi hides the launcher instead of returning to the search
    /// list — the user opened it as a standalone action, so there's no search
    /// state to go back to.
    pub(super) rofi_launched_from_hidden: bool,
    /// Set when a window-size transition begins (search ↔ Rofi): the window
    /// was dropped to alpha 0 and a `resize_window_deferred` was queued.
    /// `render` clears the flag and restores full opacity once the frame's
    /// viewport matches the target size — i.e. the deferred native resize
    /// has landed and GPUI's `viewport_size` is up to date, so the frame is
    /// painted in full and can be revealed.
    pub(super) pending_reveal: bool,
    /// How many frames we have waited for the viewport to catch up while
    /// `pending_reveal` is set. Safety net: if the viewport never matches
    /// (it should always match once the deferred resize lands), reveal
    /// anyway so the window can never be left invisible.
    pub(super) reveal_wait_frames: u32,
    /// Manager for dynamic `.dylib` plugins.
    pub(super) plugin_manager: crate::core::plugin_manager::PluginManager,
    /// Plugin search worker (started lazily on the first plugin query).
    pub(super) plugin_search: Option<PluginSearchPump>,
    /// Generation of the newest plugin search; results for older
    /// generations are dropped (superseded by a newer query, or invalidated
    /// by leaving plugin mode / a reload / a hide).
    pub(super) plugin_gen: u64,
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
    /// Pango markup parse results memoized per Rofi row text. Cleared
    /// whenever a new burst installs new rows, since the parse output
    /// depends only on the row text.
    pub(super) pango_cache:
        std::cell::RefCell<std::collections::HashMap<gpui::SharedString, PangoParse>>,
    /// Name of the active `[modes]` source filter (`None` = all sources).
    pub(super) mode_name: Option<gpui::SharedString>,
    /// Mask resolved from `mode_name`'s config value, kept so `refilter`
    /// doesn't re-parse the mode list on every keystroke.
    pub(super) mode_mask: Option<SourceMask>,
    /// Plugin entered via hotkey / `plugin:` / `mode:` (or by typing its
    /// prefix). While set, the input box shows only the query remainder
    /// after the prefix and the backspace guards below protect the prefix
    /// itself. Re-derived every `refilter` from the query text, so it
    /// cannot drift from the actual session state.
    pub(super) active_plugin: Option<std::sync::Arc<crate::core::plugin_manager::LoadedPlugin>>,
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
        let image_dims = super::helpers::collect_image_dims(&theme);
        let base_count = all.len();
        Self::validate_modes(&app_config);
        let font_fallbacks = super::helpers::font_fallback_families(&theme.font.fallback);
        let (root_font, inputbar_font, element_font_val, mono_font) =
            Self::build_fonts(&theme, &font_fallbacks);
        Self {
            all,
            filtered,
            query: String::new(),
            selected: 0,
            list: UniformListScrollHandle::new(),
            search: SearchIndex::new(&app_config.aliases, &app_config.pinned),
            history,
            app_config,
            theme: Arc::new(theme),
            state: LauncherState::Search,
            full_output_list: ListState::new(0, ListAlignment::Top, px(16.0)),
            preview_list: ListState::new(0, ListAlignment::Top, px(16.0)),
            rofi_rows_scroll: UniformListScrollHandle::new(),
            full_output_blocks: Vec::new(),
            full_output_styled: Vec::new(),
            rofi_live_search: None,
            rofi_launched_from_hidden: false,
            pending_reveal: false,
            reveal_wait_frames: 0,
            sticky_metatags: None,
            widget_registry,
            image_dims,
            last_list_viewport_h: 0.0,
            button_hotkeys,
            rofi_session: None,
            plugin_manager: crate::core::plugin_manager::PluginManager::load_all(),
            plugin_search: None,
            plugin_gen: 0,
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
            mode_name: None,
            mode_mask: None,
            active_plugin: None,
        }
    }

    /// Build the cached GPUI fonts (root, input bar, list element) from the
    /// theme. Called once per theme load so per-frame rendering only clones
    /// the (cheap) `Font` values instead of rebuilding fallback cascades.
    pub(super) fn build_fonts(
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

    /// Whether the results list is visible right now: always, unless
    /// `require_input` is set and no query matches anything yet.
    pub(super) fn list_visible(&self) -> bool {
        if self.theme.listview.require_input.unwrap_or(false) {
            !self.query.trim().is_empty() && !self.filtered.is_empty()
        } else {
            true
        }
    }

    /// Clear the query and reselect the top (first) entry.
    pub(super) fn reset(&mut self) {
        // The window stays open on the full-output page, so the search
        // list behind it must survive: keep the query and selection
        // intact until the user comes back.
        if matches!(
            self.state,
            LauncherState::RunningFull { .. } | LauncherState::FullOutput { .. }
        ) {
            return;
        }
        self.query.clear();
        self.active_plugin = None;
        self.refilter(None);
        self.selected = 0;
    }

    /// Split a `[modes]` value (comma-separated source names) into a
    /// mask. Returns `None` when no valid name is present. Silent —
    /// callers decide what to warn about.
    pub(super) fn parse_mode_list(value: &str) -> Option<SourceMask> {
        let mut mask: Option<SourceMask> = None;
        for name in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(m) = SourceMask::from_name(name) {
                mask = Some(mask.map_or(m, |acc| acc.union(m)));
            }
        }
        mask
    }

    /// Warn about `[modes]` entries that contain unknown source names or
    /// resolve to no sources at all. Runs once at startup so the user
    /// doesn't only hear about a typo when the mode is first switched to.
    fn validate_modes(config: &AppConfig) {
        for (name, value) in &config.modes {
            for source in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if SourceMask::from_name(source).is_none() {
                    eprintln!(
                        "aerofi: warning: unknown source {source:?} in [modes].{name} \
(available: apps, scripts, builtins, plugins, all)"
                    );
                }
            }
            if Self::parse_mode_list(value).is_none() {
                eprintln!(
                    "aerofi: warning: [modes].{name} resolves to no sources; \
the mode will show an empty list"
                );
            }
        }
    }

    /// Cycle the active source filter: All -> each `[modes]` entry
    /// (alphabetical, matching the config's deterministic order) -> each
    /// loaded plugin (alphabetical) -> All. A plugin mode enters that
    /// plugin's interactive session rather than applying a static filter.
    /// Switching a mode starts from an empty query, like entering a new
    /// rofi mode.
    pub(super) fn cycle_mode(&mut self, cx: Option<&mut Context<Self>>) {
        let config_modes: Vec<gpui::SharedString> = self
            .app_config
            .modes
            .keys()
            .map(|k| gpui::SharedString::from(k.clone()))
            .collect();
        let plugin_modes: Vec<gpui::SharedString> = self
            .plugin_manager
            .plugins
            .iter()
            .map(|p| p.name.clone().into())
            .collect();
        let next = match self.mode_name.as_deref() {
            None => config_modes
                .first()
                .cloned()
                .or_else(|| plugin_modes.first().cloned()),
            Some(current) => config_modes
                .iter()
                .position(|n| n == current)
                .and_then(|p| config_modes.get(p + 1).cloned())
                .or_else(|| {
                    plugin_modes
                        .iter()
                        .position(|n| n == current)
                        .and_then(|p| plugin_modes.get(p + 1).cloned())
                }),
        };
        match next {
            Some(name) => self.activate_mode(&name, cx),
            None => self.clear_mode(),
        }
    }

    /// Switch to the named mode: a `[modes]` source filter, or — when no
    /// such filter exists — a loaded plugin by name, entering its
    /// session. Unknown or source-less modes warn and leave the current
    /// mode.
    /// Switch to the named mode: a `[modes]` source filter, or — when no
    /// such filter exists — a loaded plugin by name, entering its
    /// session. `all` (any case) clears back to every source, unless the
    /// user defined a mode actually named `all`. Unknown or source-less
    /// modes warn and leave the current mode.
    pub(super) fn activate_mode(&mut self, name: &str, cx: Option<&mut Context<Self>>) {
        if name.eq_ignore_ascii_case("all")
            && !self
                .app_config
                .modes
                .keys()
                .any(|k| k.eq_ignore_ascii_case("all"))
        {
            self.clear_mode();
            return;
        }
        if let Some(value) = self.app_config.modes.get(name) {
            let Some(mask) = Self::parse_mode_list(value) else {
                eprintln!("aerofi: warning: mode {name:?} resolves to no sources");
                return;
            };
            self.mode_name = Some(gpui::SharedString::from(name.to_string()));
            self.mode_mask = Some(mask);
            self.query.clear();
            self.refilter(cx);
            self.selected = 0;
            return;
        }
        if let Some(plugin) = self.plugin_manager.find_by_name(name) {
            // A plugin without a prefix has no query-router session to
            // enter; entering a dead "mode" state would be worse than
            // warning.
            if plugin.prefix.is_empty() {
                eprintln!(
                    "aerofi: warning: plugin {name:?} has no prefix and \
cannot be entered as a mode"
                );
                return;
            }
            let prefix = plugin.prefix.clone();
            self.open_plugin(cx, &prefix);
            return;
        }
        eprintln!(
            "aerofi: warning: unknown mode {name:?} (not in [modes] and \
no plugin by that name)"
        );
    }

    /// Back to all sources (clearing the query, like any mode switch).
    fn clear_mode(&mut self) {
        if self.mode_name.is_none() {
            return;
        }
        self.mode_name = None;
        self.mode_mask = None;
        self.query.clear();
        self.refilter(None);
        self.selected = 0;
    }

    /// Text shown in the input bar. In an active plugin session the
    /// prefix is state, not user text, so only the query remainder is
    /// displayed — an empty box means "show the plugin's default
    /// results".
    pub(super) fn display_query(&self) -> &str {
        if let Some(p) = &self.active_plugin
            && let Some(remainder) = self.query.strip_prefix(&p.prefix)
        {
            return remainder;
        }
        &self.query
    }

    pub(super) fn backspace(&mut self, cx: Option<&mut Context<Self>>) {
        // The plugin prefix is the session anchor, not user text: an
        // active session with an empty remainder can't be backspaced
        // into (it would corrupt the anchor). Clear/reset exits instead.
        if let Some(p) = &self.active_plugin
            && self.query == p.prefix
        {
            return;
        }
        if self.query.pop().is_some() {
            self.refilter(cx);
            self.selected = 0;
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
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
    pub(super) fn refilter(&mut self, cx: Option<&mut Context<Self>>) {
        // A new query changes every row's highlight ranges; drop the memo.
        self.highlight_cache.borrow_mut().clear();

        if let Some((plugin, remainder)) = self.plugin_manager.match_prefix(&self.query) {
            let remainder = remainder.to_string();
            // The session is live: remember the plugin so the input bar
            // can hide the prefix anchor from the displayed text.
            self.active_plugin = Some(plugin.clone());

            if let Some(cx) = cx {
                // Keep the previous plugin rows (and the `filtered` pointing
                // at them) until the debounced results replace them. Clearing
                // the list for the ~90 ms + process round-trip left
                // `filtered` briefly empty, and require_input themes then
                // collapsed the window to the bar on every plugin keystroke
                // — the "flicker". `filtered` stays valid because the old
                // rows are not truncated here; the result-apply below swaps
                // them in atomically.
                if self.all.len() <= self.base_count {
                    // First plugin query: no previous rows to keep — show
                    // base app/script matches until the results arrive.
                    self.search.search(
                        &self.query,
                        &self.all[..self.base_count],
                        &self.history,
                        self.app_config.general.matching,
                        self.app_config.general.ranking,
                        self.mode_mask,
                        &mut self.filtered,
                    );
                    self.filtered.truncate(self.app_config.general.max_results);
                }

                // Hand the query to the single coalescing worker instead of
                // spawning a thread + plugin process per typing pause. Only
                // the newest request of a burst is ever executed, at most
                // one process runs at a time, and superseded results are
                // dropped by generation — no orphaned subprocesses.
                if self.plugin_search.is_none() {
                    self.plugin_search = Some(Self::start_plugin_search_pump(cx.entity(), cx));
                }
                self.plugin_gen = self.plugin_gen.wrapping_add(1);
                let _ = self
                    .plugin_search
                    .as_mut()
                    .expect("pump just started")
                    .tx
                    .send(PluginSearchRequest {
                        generation: self.plugin_gen,
                        plugin,
                        remainder,
                    });
            } else {
                // A synchronous query applies its result directly; invalidate
                // anything still in flight on the async worker.
                self.plugin_gen = self.plugin_gen.wrapping_add(1);
                self.all.truncate(self.base_count);
                let parsed = plugin.query_parsed(&remainder);
                self.all.extend(parsed);

                // For plugins, we don't fuzzy sort. We show exactly what the plugin returned in order.
                self.filtered = (self.base_count..self.all.len()).collect();
            }
        } else {
            // Left plugin mode: invalidate any pending async result.
            self.active_plugin = None;
            self.plugin_gen = self.plugin_gen.wrapping_add(1);
            // Clear any previous plugin items and release excess capacity.
            self.all.truncate(self.base_count);
            if self.all.capacity() > self.base_count * 2 {
                self.all.shrink_to(self.base_count);
            }
            self.search.search(
                &self.query,
                &self.all[..self.base_count],
                &self.history,
                self.app_config.general.matching,
                self.app_config.general.ranking,
                self.mode_mask,
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
        // Kill any active Rofi session.
        if let Some(session) = self.rofi_session.take()
            && let Ok(mut s) = session.lock()
        {
            s.kill();
        }
        // Stop the live-search pump thread (dropping the sender makes its
        // recv() return and the thread exits).
        self.rofi_live_search.take();
        // Invalidate in-flight plugin searches: the next show re-filters
        // synchronously, so a stale result arriving late must not overwrite
        // the fresh state.
        self.plugin_gen = self.plugin_gen.wrapping_add(1);
        // Drop the last session's memoized Pango parses (see `rofi_leave`).
        self.pango_cache.borrow_mut().clear();
        self.sticky_metatags = None;
        // Cancel any in-flight size transition: hiding during a pending
        // reveal would otherwise leave the flag set across the hide/show
        // cycle, and a stale safety-net timer could fire on a hidden window.
        self.pending_reveal = false;
        self.reveal_wait_frames = 0;
        crate::sys::appkit::invalidate_reveal_safety_net();
        // Cancelling the reveal dance must also undo its alpha drop —
        // otherwise a reload whose reveal was cancelled here would leave
        // `alpha = 0` with nothing ever restoring it, and the next show
        // would present an invisible window.
        crate::sys::appkit::set_window_alpha(1.0);
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
        // Visibility contract: if no reveal dance is in flight, the window
        // must not be left transparent — a reload's alpha-0 whose reveal was
        // cancelled by a hide would otherwise survive into this show and the
        // user would see nothing. A pending dance skips this and reveals via
        // the normal render path once the frame has landed.
        if !self.pending_reveal {
            crate::sys::appkit::set_window_alpha(1.0);
        }
        // Inline daemons buffered their ticks while the window was hidden
        // (the main thread was never woken for them); apply the latest of
        // each before the first frame so subtitles aren't stale.
        for (path, text) in crate::core::scheduler::flush_pending_inline() {
            self.apply_inline_output(&path, Some(gpui::SharedString::from(text)));
        }
        // Source-folder changes that arrived while the window was hidden
        // (the main thread was never woken for them); apply them before
        // the first frame so the list isn't stale.
        let (apps, scripts, plugins) = crate::core::fs_watcher::take_pending();
        if apps {
            self.rescan_apps(None);
        }
        if scripts {
            self.rescan_scripts();
        }
        if plugins {
            self.rescan_plugins();
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

                    let mut initial_values = vec![String::new(); len];
                    Self::prefill_dropdown(&mut initial_values, &arg_clones, 0);

                    self.state = LauncherState::ArgumentInput {
                        target: item.clone(),
                        args: arg_clones,
                        values: initial_values,
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
                if new_instance {
                    match self.app_config.general.new_instance_mode {
                        NewInstanceMode::Smart => {
                            crate::core::executor::smart_open_app(path, item.name())
                        }
                        NewInstanceMode::Force => {
                            crate::core::executor::open_app(path, item.name(), true, false)
                        }
                        NewInstanceMode::Background => {
                            crate::core::executor::open_app(path, item.name(), true, true)
                        }
                    }
                } else {
                    crate::core::executor::open_app(path, item.name(), false, false);
                }
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
                    ScriptMode::Rofi => LauncherAction::StartRofiSession(item.clone(), args),
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
            LauncherAction::StartRofiSession(target, args) => {
                self.start_rofi_session(&target, args, cx);
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
        // Full output keeps the window open, so preserve the query and
        // selection for when the user comes back.
        if matches!(
            action,
            LauncherAction::Hide | LauncherAction::ExecuteScript(..)
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

    /// When a freshly-focused dropdown argument is still empty, preselect its
    /// first option so the user has a valid value to start from.
    pub(super) fn prefill_dropdown(
        values: &mut [String],
        args: &[crate::core::item::ScriptArgument],
        ix: usize,
    ) {
        if args[ix].arg_type.as_deref() == Some("dropdown")
            && values[ix].is_empty()
            && let Some(first_opt) = args[ix].data.as_deref().and_then(|d| d.first())
        {
            values[ix] = first_opt.value.clone();
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
                target.set_inline_output(output.clone().map(|s| s.to_string()));
            }
        }
    }

    /// Open a plugin by prefix: reset any transient state (Rofi session,
    /// full output, argument input) and put the launcher in the plugin's
    /// query mode — the same state the user would reach by typing the
    /// prefix. Called from `window::open_plugin` (a global hotkey) and
    /// from the `plugin:` / `mode:` button actions.
    pub(crate) fn open_plugin(&mut self, cx: Option<&mut Context<Self>>, prefix: &str) {
        match &self.state {
            LauncherState::RofiMode { .. } => {
                // May hide the window again (a session launched from a
                // hidden hotkey); the caller re-shows it if needed.
                self.rofi_leave();
            }
            _ => {
                self.back_from_full_output();
                if !matches!(self.state, LauncherState::Search) {
                    // ArgumentInput / Confirming: drop the transient dialog.
                    self.state = LauncherState::Search;
                }
            }
        }
        self.query = prefix.to_string();
        // A session opened from outside is a mode too: show the plugin's
        // name in the input-bar badge and lift any static source filter.
        let plugin = self.plugin_manager.match_prefix(prefix).map(|(p, _)| p);
        if let Some(p) = &plugin {
            self.mode_name = Some(gpui::SharedString::from(p.name.clone()));
        }
        self.mode_mask = None;
        self.active_plugin = plugin;
        self.refilter(cx);
        self.selected = 0;
        println!("aerofi: opened plugin mode (prefix: {prefix:?})");
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

        // Open a plugin by name. The launcher is the plugin's UI, so the
        // window stays open regardless of the button's `close` attribute.
        if let Some(plugin_name) = trimmed.strip_prefix("plugin:") {
            let plugin_name = plugin_name.trim();
            match self.plugin_manager.find_by_name(plugin_name) {
                Some(plugin) => {
                    self.open_plugin(Some(&mut *cx), &plugin.prefix);
                    cx.notify();
                }
                None => eprintln!("aerofi: warning: button plugin not found: {plugin_name}"),
            }
            return;
        }

        // Switch to a named source filter ([modes]). Like plugin:, the
        // launcher is its own UI, so the window stays open regardless of
        // the button's `close` attribute.
        // Switch to a named mode: a `[modes]` source filter, or a loaded
        // plugin by name (entering its session). Like plugin:, the
        // launcher is its own UI, so the window stays open regardless of
        // the button's `close` attribute.
        if let Some(mode_name) = trimmed.strip_prefix("mode:") {
            let mode_name = mode_name.trim().to_string();
            self.activate_mode(&mode_name, Some(&mut *cx));
            cx.notify();
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
}
