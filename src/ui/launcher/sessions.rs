use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Context, ScrollStrategy};

use crate::core::item::{ScriptMetatags, Target};
use crate::core::rofi_protocol::RofiCommand;
use crate::core::rofi_session::{self, ReadResult, RofiSession};

use super::helpers::combo_matches;
use super::state::Launcher;
use super::types::{LauncherAction, LauncherState};

impl Launcher {
    pub(super) fn start_rofi_session(
        &mut self,
        target: &Target,
        args: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Target::Script { path, name, .. } = target else {
            return;
        };
        let title = name.to_string();
        let layout = target.metatags().and_then(|m| m.layout.clone());

        // Metatags are committed only when we actually enter Rofi mode (see
        // `show_rofi_loading` / the burst task below) — not here — so the
        // search list doesn't render with the Rofi layout (columns, hidden
        // input bar) during the brief wait for the first burst.
        let metatags = target.metatags().cloned();

        // Record in history.
        let identifier = target.identifier();
        self.history.record_launch(identifier);

        let path = path.clone();
        let mut envs = std::collections::HashMap::new();
        envs.insert("AEROFI_RETV".to_string(), "0".to_string());

        match RofiSession::spawn(&path, args, envs) {
            Ok(session) => {
                let session = Arc::new(Mutex::new(session));
                // A fresh session gets a fresh live-search pump (the old one
                // is bound to the previous session's pipes).
                self.rofi_live_search.take();
                self.rofi_session = Some(session.clone());

                // Read initial burst on a background thread, then update UI.
                let view = cx.entity();
                let title2 = title.clone();
                let cx_async = cx.to_async();
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = std::thread::Builder::new()
                    .name("aerofi-rofi-init".to_string())
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
                                // Commit the layout metatags as we enter Rofi
                                // mode. If the script exited without output,
                                // `rofi_leave()` below clears them again.
                                launcher.sticky_metatags = metatags_burst;
                                launcher.rofi_handle_read_result(burst, &title2);
                                cx.notify();
                            });
                        });
                    }
                })
                .detach();

                // Show a "loading" frame only if the script is slow to emit
                // its first burst. Fast scripts go straight from the search
                // list to the populated Rofi, so the user never sees an empty
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
                                    launcher.show_rofi_loading(
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
                eprintln!("aerofi: failed to start Rofi session for {title}: {e}");
            }
        }
    }

    /// Show the temporary "loading" Rofi frame, but only if a burst hasn't
    /// landed yet (we're still not in `RofiMode`) and the session is still
    /// alive. Invoked by a delayed task from [`start_rofi_session`], so fast
    /// scripts never trigger it — the user goes straight from the search list
    /// to the populated Rofi with no empty flash.
    fn show_rofi_loading(
        &mut self,
        title: &str,
        layout: Option<String>,
        metatags: Option<ScriptMetatags>,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.state, LauncherState::RofiMode { .. }) || self.rofi_session.is_none() {
            return;
        }
        // A new Rofi session starts with fresh rows.
        self.pango_cache.borrow_mut().clear();
        // Commit the layout metatags as we enter Rofi mode (loading frame).
        self.sticky_metatags = metatags;
        self.state = LauncherState::RofiMode {
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
        // This only runs when we're not already in RofiMode, so it's always a
        // search → Rofi transition: prepare the window (resize + hide stale
        // content) so the loading frame appears at its final size.
        self.reveal_rofi_transition();
    }

    /// Prepare the window for a search → Rofi transition: resize it to the Rofi
    /// layout and hide the stale content (the search list, or a previous
    /// frame) at alpha 0 until the first Rofi render reveals it. This hides the
    /// otherwise-visible "resize on the go" for both entry paths — a hotkey
    /// launch (window hidden, so also shown here) and picking the script from
    /// the search list (window already visible at the search size).
    ///
    /// Called from within an `App` update (we hold `self`), so it must not
    /// re-enter the App: `on_show` and `show_window` run directly, without
    /// `App::update`.
    fn reveal_rofi_transition(&mut self) {
        let screen_h = crate::sys::appkit::screen_height();
        let height = self.rofi_fit_height(screen_h);
        let screen_w = crate::sys::appkit::screen_width();
        let width = self.rofi_fit_width(screen_w);
        let t = self.theme.clone();
        let from_hidden = !crate::ui::window::is_visible();
        // Hide the stale content *before* resizing, so the window vanishes
        // instead of visibly shrinking from the search size to the Rofi size.
        // The resize itself is deferred to the main queue (see
        // `resize_window_deferred` — it must not run inside this App update,
        // or GPUI's resize callback fails to update the viewport); `render`
        // restores opacity once the viewport matches the new size.
        crate::sys::appkit::set_window_alpha(0.0);
        self.pending_reveal = true;
        self.reveal_wait_frames = 0;
        crate::sys::appkit::arm_reveal_safety_net(120);
        crate::sys::appkit::resize_window_deferred(
            width as f64,
            height as f64,
            t.window.x_offset as f64,
            t.window.y_offset as f64,
        );
        // If the window is hidden (the script was launched from a global
        // hotkey while the app was hidden), show it now — still at alpha 0.
        if from_hidden {
            crate::core::scheduler::notify_visibility(true);
            crate::core::fs_watcher::notify_visibility(true);
            self.on_show();
            crate::ui::window::show_window();
        }
        // Remember how we entered so `rofi_leave` can decide whether to hide
        // (launched from a hotkey) or return to the search list (launched from
        // the visible search).
        self.rofi_launched_from_hidden = from_hidden;
    }

    /// Handle the read result from a Rofi session burst.
    fn rofi_handle_read_result(&mut self, result: ReadResult, title: &str) {
        match result {
            ReadResult::Burst(burst) => {
                self.rofi_apply_burst(burst, title);
            }
            ReadResult::BurstThenExit(burst) => {
                self.rofi_apply_burst(burst, title);
                // Script exited after this burst — mark session as done and
                // drop its live-search pump (a new session must not reuse it).
                // The UI will stay showing the last rows but selecting will
                // leave Rofi mode.
                self.rofi_session = None;
                self.rofi_live_search.take();
            }
            ReadResult::Exited => {
                // Script exited with no output — return to search.
                self.rofi_leave();
            }
            ReadResult::Error(e) => {
                eprintln!("aerofi: Rofi script error: {e}");
                self.rofi_leave();
            }
        }
    }

    /// Apply a Rofi burst (commands + rows) to the current RofiMode state.
    pub(super) fn rofi_apply_burst(
        &mut self,
        burst: crate::core::rofi_protocol::RofiBurst,
        title: &str,
    ) {
        // Whether we're updating an existing Rofi session (live-search burst)
        // or entering Rofi mode for the first time (search → Rofi transition).
        let was_rofi = matches!(self.state, LauncherState::RofiMode { .. });
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

        // Inherit flags if we are updating an existing RofiMode session
        if let LauncherState::RofiMode {
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

        // Fall back to the script's `@aerofi.preset` (committed to
        // `sticky_metatags` when the session starts). A fast script emits its
        // first burst before the 120ms loading frame, so `show_rofi_loading`
        // — the only other place that seeds `layout` — never runs and the
        // preset (element sizes, columns) would otherwise be dropped.
        if layout.is_none() {
            layout = self.sticky_metatags.as_ref().and_then(|m| m.layout.clone());
        }

        for cmd in &burst.commands {
            match cmd {
                RofiCommand::Flush => {}
                RofiCommand::SetPrompt(p) => prompt = Some(p.clone()),
                RofiCommand::SetMessage(m) => message = Some(m.clone()),
                RofiCommand::EnableMarkup => markup_rows = true,
                RofiCommand::NoCustom(v) => no_custom = *v,
                RofiCommand::KeepSelection(s) => keep_selection = Some(s.clone()),
                RofiCommand::SetColumns(n) => columns = Some(*n),
                RofiCommand::SetLoading(l) => loading = *l,
                RofiCommand::LiveSearch(ls) => live_search = *ls,
                RofiCommand::SetActiveIndices(indices) => active_indices = indices.clone(),
                RofiCommand::SetData(d) => data = Some(d.clone()),
                RofiCommand::PreviewText(text) => {
                    let blocks = crate::core::markdown::parse(text);
                    preview_styled = Some(blocks.iter().map(|b| self.build_md_styled(b)).collect());
                    preview_blocks = Some(blocks);
                    preview_changed = true;
                }
                RofiCommand::PreviewFile(file_path) => {
                    let resolved = super::helpers::expand_tilde_path(file_path);
                    if let Ok(text) = std::fs::read_to_string(&resolved) {
                        let blocks = crate::core::markdown::parse(&text);
                        preview_styled =
                            Some(blocks.iter().map(|b| self.build_md_styled(b)).collect());
                        preview_blocks = Some(blocks);
                        preview_changed = true;
                    }
                }
                RofiCommand::MultiSelect(b) => multi_select = *b,
                RofiCommand::MarkupRows(b) => markup_rows = *b,
                RofiCommand::Reload => {
                    // Leave Rofi mode before reloading so the user sees the
                    // regular search list immediately after the script exits.
                    // We must return early to avoid the `self.state = RofiMode`
                    // assignment at the bottom of this function overwriting the
                    // `Search` state that rofi_leave() sets.
                    // rofi_leave() already reloaded when the theme changed on
                    // disk; only reload again for other config changes so a
                    // theme switch triggers exactly one full re-index.
                    if !self.rofi_leave() {
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

        self.state = LauncherState::RofiMode {
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

        // On a search → Rofi transition (first burst), prepare the window so
        // the resize is hidden and the first visible frame is the Rofi — for
        // both a hotkey launch (window hidden) and picking the script from the
        // search list (window already visible at the search size). Live-search
        // bursts (already in RofiMode) skip this to avoid an alpha flicker.
        if !was_rofi {
            self.reveal_rofi_transition();
        }
    }

    /// Handle keystrokes while in `LauncherState::RofiMode`.
    pub(super) fn handle_rofi_mode_keystroke(
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
                self.rofi_dispatch_selection(cx, "custom", Some(retv));
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

        // With the search bar hidden (`@aerofi.show_search false`) there is
        // nothing to type into, so character input (and backspace) is ignored.
        let show_search = self
            .sticky_metatags
            .as_ref()
            .and_then(|m| m.show_search)
            .unwrap_or(true);

        match (ks.key.as_str(), cmd, ctrl, alt, shift) {
            ("escape", false, false, false, false) => {
                self.rofi_leave();
                LauncherAction::None
            }
            ("enter" | "return", false, false, false, false) => {
                if let Some(cx) = cx {
                    self.rofi_select_row(cx);
                }
                LauncherAction::None
            }
            ("enter" | "return", false, false, false, true) => {
                // Shift+Enter contextual action
                if let Some(cx) = cx {
                    self.rofi_action_row(cx, "shift+enter");
                }
                LauncherAction::None
            }
            ("enter" | "return", false, false, true, false) => {
                // Alt+Enter contextual action
                if let Some(cx) = cx {
                    self.rofi_action_row(cx, "alt+enter");
                }
                LauncherAction::None
            }
            ("up", false, false, false, false)
            | ("p", false, true, false, false)
            | ("k", false, true, false, false) => {
                let cols = self.rofi_columns() as isize;
                self.rofi_move_selection(-cols);
                LauncherAction::None
            }
            ("down", false, false, false, false)
            | ("n", false, true, false, false)
            | ("j", false, true, false, false) => {
                let cols = self.rofi_columns() as isize;
                self.rofi_move_selection(cols);
                LauncherAction::None
            }
            ("up", true, false, false, false) => {
                self.rofi_jump_to_edge(true);
                LauncherAction::None
            }
            ("down", true, false, false, false) => {
                self.rofi_jump_to_edge(false);
                LauncherAction::None
            }
            ("left", false, false, false, false) | ("b", false, true, false, false) => {
                self.rofi_move_selection(-1);
                LauncherAction::None
            }
            ("right", false, false, false, false) | ("f", false, true, false, false) => {
                self.rofi_move_selection(1);
                LauncherAction::None
            }
            ("tab", false, false, false, false) => {
                self.rofi_toggle_selection(1);
                LauncherAction::None
            }
            ("tab", false, false, false, true) => {
                self.rofi_toggle_selection(-1);
                LauncherAction::None
            }
            ("backspace", false, false, false, false) => {
                let mut is_live = false;
                if show_search
                    && let LauncherState::RofiMode {
                        query, live_search, ..
                    } = &mut self.state
                {
                    is_live = *live_search;
                    if query.pop().is_some() && !is_live {
                        self.rofi_refilter();
                    }
                }
                if is_live && let Some(cx) = cx {
                    self.rofi_send_live_search(cx);
                }
                LauncherAction::None
            }
            (key, false, true, false, false) => {
                // Ctrl + <key> contextual action (e.g. ctrl+e, ctrl+d)
                if let Some(cx) = cx {
                    let action_key = format!("ctrl+{key}");
                    self.rofi_action_row(cx, &action_key);
                }
                LauncherAction::None
            }
            _ => {
                if show_search
                    && !cmd
                    && !ctrl
                    && !alt
                    && ks.key != "tab"
                    && let Some(c) = ks.key_char.as_deref()
                    && !c.is_empty()
                    && !c.chars().any(char::is_control)
                {
                    let mut is_live = false;
                    if let LauncherState::RofiMode {
                        query, live_search, ..
                    } = &mut self.state
                    {
                        query.push_str(c);
                        is_live = *live_search;
                        if !is_live {
                            self.rofi_refilter();
                        }
                    }
                    if is_live && let Some(cx) = cx {
                        self.rofi_send_live_search(cx);
                    }
                }
                LauncherAction::None
            }
        }
    }

    /// Move the selection cursor within the filtered Rofi rows.
    fn rofi_move_selection(&mut self, delta: isize) {
        let (new_selected, row_count) = if let LauncherState::RofiMode {
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
            (*selected, filtered_rows.len())
        } else {
            return;
        };
        // Keep the selected row in view as the user navigates with the arrows.
        // In grid mode the list items are rows of `cols` cells, so convert
        // the flat selection index to the grid-row index first.
        let cols = self.rofi_columns().max(1);
        let total_rows = row_count.div_ceil(cols);
        // A single-row grid fills the (stretched) list exactly, so there is
        // nothing to scroll — and `scroll_to_item` on it nags the list's
        // scroll offset, making the row's centred content jitter on every
        // selection change.
        if total_rows > 1 {
            let item_ix = if cols > 1 {
                new_selected / cols
            } else {
                new_selected
            };
            self.rofi_rows_scroll
                .scroll_to_item(item_ix, ScrollStrategy::Nearest);
        }
    }

    fn rofi_jump_to_edge(&mut self, top: bool) {
        let new_selected = if let LauncherState::RofiMode {
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
        let cols = self.rofi_columns().max(1);
        let item_ix = if cols > 1 {
            new_selected / cols
        } else {
            new_selected
        };
        self.rofi_rows_scroll
            .scroll_to_item(item_ix, ScrollStrategy::Nearest);
    }

    /// Toggle selection of the currently focused row if multi-select is enabled.
    fn rofi_toggle_selection(&mut self, delta: isize) {
        let selected_copy = if let LauncherState::RofiMode {
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
            self.rofi_move_selection(delta);
        }
    }

    pub(super) fn rofi_columns(&self) -> usize {
        if let LauncherState::RofiMode { columns, .. } = &self.state {
            (*columns).unwrap_or(1)
        } else {
            1
        }
    }

    /// User selected a row in Rofi mode with standard Enter.
    pub(super) fn rofi_select_row(&mut self, cx: &mut Context<Self>) {
        self.rofi_dispatch_selection(cx, "enter", None);
    }

    /// User triggered a contextual action key on the selected row in Rofi mode.
    pub(super) fn rofi_action_row(&mut self, cx: &mut Context<Self>, key: &str) {
        self.rofi_dispatch_selection(cx, key, Some(10));
    }

    /// Dispatch either a Select, Action, or Custom input event to the Rofi script's stdin.
    fn rofi_dispatch_selection(
        &mut self,
        cx: &mut Context<Self>,
        key: &str,
        action_retv: Option<i32>,
    ) {
        let LauncherState::RofiMode {
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
                crate::core::rofi_protocol::RofiEvent::Action {
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
                crate::core::rofi_protocol::RofiEvent::Select {
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
            crate::core::rofi_protocol::RofiEvent::Custom {
                key: key.to_string(),
                text: query.clone(),
                retv: 2,
                data: data.clone(),
            }
        } else {
            return;
        };

        let Some(session) = self.rofi_session.clone() else {
            self.rofi_leave();
            return;
        };

        // Mark loading state while waiting for the next response
        if let LauncherState::RofiMode { loading, .. } = &mut self.state {
            *loading = true;
        }

        let view = cx.entity();
        let cx_async = cx.to_async();
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = std::thread::Builder::new()
            .name("aerofi-rofi-event".to_string())
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
                        launcher.rofi_handle_read_result(result, &title);
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
    fn rofi_send_live_search(&mut self, cx: &mut Context<Self>) {
        let LauncherState::RofiMode {
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
        let Some(session) = self.rofi_session.clone() else {
            return;
        };

        // Mark loading while script recalculates
        if let LauncherState::RofiMode { loading, .. } = &mut self.state {
            *loading = true;
        }

        // Start the pump lazily on the first live-search keystroke.
        let pump = match self.rofi_live_search.as_mut() {
            Some(p) => p,
            None => {
                let pump = Self::start_live_search_pump(session, title.clone(), cx.entity(), cx);
                self.rofi_live_search = Some(pump);
                self.rofi_live_search.as_mut().expect("pump just started")
            }
        };
        let query_generation = pump.latest_enqueued.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = pump.tx.send((query, query_generation));
    }

    /// Spawn the live-search pump thread for `session` and return its
    /// control handle. The thread exits when the returned `LiveSearchPump`
    /// is dropped (its sender half).
    fn start_live_search_pump(
        session: Arc<Mutex<RofiSession>>,
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
            .name("aerofi-rofi-live-search".to_string())
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
                        // active (left Rofi mode, or a new session started)
                        // and skip frames superseded by a newer query.
                        let active = launcher
                            .rofi_session
                            .as_ref()
                            .is_some_and(|s| Arc::ptr_eq(s, &session));
                        if active && !stale {
                            if let Some(result) = result {
                                launcher.rofi_handle_read_result(result, &title);
                            }
                            if let LauncherState::RofiMode { loading, .. } = &mut launcher.state {
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

    /// Spawn the plugin search worker thread and return its control handle.
    /// The thread exits when the returned `PluginSearchPump` is dropped
    /// (its sender half), mirroring [`Self::start_live_search_pump`].
    pub(super) fn start_plugin_search_pump(
        view: gpui::Entity<Launcher>,
        cx: &mut Context<Self>,
    ) -> PluginSearchPump {
        let (query_tx, query_rx) = mpsc::channel::<PluginSearchRequest>();
        // The worker is a plain OS thread (it blocks on the channel and runs
        // plugin subprocesses), so it can't hold GPUI handles; finished
        // results are handed to a GPUI-side task over this channel. Dropping
        // `result_tx` (when the worker exits) ends the task.
        let (mut result_tx, mut result_rx) =
            futures::channel::mpsc::channel::<PluginSearchApply>(8);

        let _ = std::thread::Builder::new()
            .name("aerofi-plugin-search".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                // Coalesce to the newest request: block for the first one,
                // then absorb anything newer within the 90 ms typing-pause
                // window. Only the latest query of a burst is ever executed,
                // so at most one plugin subprocess runs at a time and none
                // is left orphaned with a result nobody applies.
                let debounce = Duration::from_millis(90);
                loop {
                    let req = match query_rx.recv() {
                        Ok(req) => req,
                        Err(_) => break, // handle dropped
                    };
                    let mut req = req;
                    let deadline = Instant::now() + debounce;
                    while let Ok(later) =
                        query_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    {
                        req = later;
                    }

                    let mut parsed = req.plugin.query_parsed(&req.remainder);
                    // Swap full-resolution image paths for bounded
                    // thumbnails before the rows hit the renderer (first use
                    // decodes here, off the main thread).
                    crate::sys::icons::thumbnail_image_icons(&mut parsed);
                    if result_tx
                        .try_send(PluginSearchApply {
                            generation: req.generation,
                            parsed,
                        })
                        .is_err()
                    {
                        break; // GPUI side is gone
                    }
                }
            });

        let cx_async = cx.to_async();
        cx.spawn(move |_, _: &mut gpui::AsyncApp| async move {
            while let Ok(apply) = result_rx.recv().await {
                cx_async.update(|cx| {
                    view.update(cx, |launcher, cx| {
                        // Drop superseded or invalidated results (a newer
                        // query was enqueued, or the search state was reset
                        // by leaving plugin mode / a reload / a hide).
                        if apply.generation != launcher.plugin_gen {
                            return;
                        }
                        launcher.all.truncate(launcher.base_count);
                        launcher.all.extend(apply.parsed);
                        launcher.filtered = (launcher.base_count..launcher.all.len()).collect();
                        if launcher.selected >= launcher.filtered.len() {
                            launcher.selected = 0;
                        }
                        launcher.list.scroll_to_item(0, ScrollStrategy::Top);
                        cx.notify();
                    });
                });
            }
        })
        .detach();

        PluginSearchPump { tx: query_tx }
    }

    /// Send a live-search query to the script, then drain its output until
    /// the script goes quiet. Returns `None` if the script produced nothing
    /// within the first-line deadline (the UI stays on its current rows).
    fn send_and_drain(session: &Mutex<RofiSession>, query: &str) -> Option<ReadResult> {
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
        // Hard stop for the whole drain. A well-behaved script emits its
        // frame and then blocks waiting for input, so the loop normally ends
        // on the first quiet chunk. But a script that streams without ever
        // going quiet would otherwise spin here forever, pinning this thread
        // (and the session `Arc` it holds) — so if the session is later
        // replaced, the old child would never be killed. Return the last
        // frame we saw once this budget is exhausted.
        let total_deadline = Instant::now() + Duration::from_secs(5);
        let mut last: Option<ReadResult> = None;
        loop {
            if Instant::now() >= total_deadline {
                break;
            }
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

    /// Re-filter Rofi rows based on the current query.
    fn rofi_refilter(&mut self) {
        if let LauncherState::RofiMode {
            rows,
            filtered_rows,
            query,
            selected,
            ..
        } = &mut self.state
        {
            *filtered_rows = rofi_session::filter_rofi_rows(rows, query);
            if *selected >= filtered_rows.len() {
                *selected = 0;
            }
        }
    }

    /// Leave Rofi mode: kill the session and return to the search list.
    /// Leave Rofi mode. Returns `true` if a reload was performed because the
    /// theme changed on disk (callers that also want a reload — e.g. the
    /// `Reload` Rofi command — can skip their own to avoid a double re-index).
    pub(super) fn rofi_leave(&mut self) -> bool {
        // A Rofi session launched from a hidden window (a global hotkey) was
        // opened as a standalone action, so there's no search list to return
        // to — hide the launcher and return focus to the previous app.
        // (Set by `reveal_rofi_transition`, based on whether the window was
        // visible when the session started.)
        let hide_on_exit = self.rofi_launched_from_hidden;

        if let Some(session) = self.rofi_session.take()
            && let Ok(mut s) = session.lock()
        {
            s.kill();
        }
        // Stop the live-search pump thread for this session.
        self.rofi_live_search.take();
        // Clear sticky_metatags so the Rofi script's layout overrides (e.g.
        // `@aerofi.columns 1`) don't linger after the script exits.
        // Without this, a 4-column grid theme would render as a 1-column list
        // after closing a rofi-mode script that declared columns = 1.
        self.sticky_metatags = None;
        let new_config = crate::core::config::AppConfig::load();
        let reloaded = new_config.theme != self.app_config.theme;
        if reloaded {
            self.reload();
        }
        self.state = LauncherState::Search;
        // The Pango cache is keyed by the session's row texts; drop it so the
        // last session's parse results (and their text keys) don't stay
        // resident until the next session happens to start.
        self.pango_cache.borrow_mut().clear();

        // Fire-and-forget Rofi launch: hide the launcher and return focus to
        // the previously active app instead of leaving the search list up.
        if hide_on_exit {
            self.on_hide();
            crate::ui::window::hide();
            return reloaded;
        }

        // Drop the window to invisible and defer the resize/re-center to the
        // main queue (outside this App update — see `resize_window_deferred`,
        // which must not run here or GPUI's viewport stays stale). `render`
        // restores opacity once the viewport matches the search dimensions,
        // so the size/position change is never visible.
        let t = self.theme.clone();
        let screen_w = crate::sys::appkit::screen_width();
        let screen_h = crate::sys::appkit::screen_height();
        crate::sys::appkit::set_window_alpha(0.0);
        self.pending_reveal = true;
        self.reveal_wait_frames = 0;
        crate::sys::appkit::arm_reveal_safety_net(120);
        let width = t.window.width.resolve(screen_w).min(screen_w);
        let height = t.window.height.resolve(screen_h).min(screen_h);
        crate::sys::appkit::resize_window_deferred(
            width as f64,
            height as f64,
            t.window.x_offset as f64,
            t.window.y_offset as f64,
        );

        reloaded
    }
}
///
/// Dropping the value (on `rofi_leave` / `on_hide`) drops the sender half,
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
    session: Arc<Mutex<RofiSession>>,
    /// `true` if a newer query was enqueued while this round was draining.
    stale: bool,
}

/// Handle for the single plugin search worker thread.
///
/// Dropping the value (with the `Launcher`) drops the sender half, which
/// makes the worker's `recv()` return and the thread exit.
pub(super) struct PluginSearchPump {
    /// Sender half: each plugin query pushes a request.
    pub(super) tx: mpsc::Sender<PluginSearchRequest>,
}

/// One enqueued plugin query, handed to the worker thread.
pub(super) struct PluginSearchRequest {
    /// `Launcher::plugin_gen` at enqueue time; the GPUI side drops results
    /// for anything older.
    pub(super) generation: u64,
    pub(super) plugin: Arc<crate::core::plugin_manager::LoadedPlugin>,
    pub(super) remainder: String,
}

/// One finished plugin search round, handed from the worker thread to the
/// GPUI-side task that applies it on the main thread.
struct PluginSearchApply {
    generation: u64,
    parsed: Vec<Target>,
}
