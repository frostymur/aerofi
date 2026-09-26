//! GPUI rendering for the launcher: the `Render` impl and all private
//! view-building helpers (inputbar, listview, grid, full-output, markdown).

use gpui::{
    Context, CursorStyle, Render, Window, WindowBackgroundAppearance, div, img, prelude::*, px,
    rgba, size, uniform_list,
};

use crate::core::item::Target;
use crate::core::theme::{BuiltinWidget, ThemeConfig, Widget, parse_hex_color_alpha};

use super::helpers::{
    apply_md_style, expand_tilde_path, format_combo, is_image_path, is_primary_click,
};
use super::state::Launcher;
use super::types::{LauncherState, MdStyled};

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();

        // Layout overrides come only from the last executed script
        // (sticky until the launcher hides); selection never applies them.
        let show_search = self
            .sticky_metatags
            .as_ref()
            .and_then(|m| m.show_search)
            .unwrap_or(true);
        let columns = self.effective_columns();

        // Determine whether the list should be visible.
        let require_input = t.listview.require_input.unwrap_or(false);
        let auto_height = t.window.auto_height.unwrap_or(require_input);
        let should_show_list = if require_input {
            !self.query.trim().is_empty() && !self.filtered.is_empty()
        } else {
            true
        };

        let ib_height = t.inputbar.height;

        let screen_w = window
            .display(cx)
            .map(|d| d.bounds().size.width.as_f32())
            .unwrap_or(1920.0);
        let screen_h = window
            .display(cx)
            .map(|d| d.bounds().size.height.as_f32())
            .unwrap_or(1080.0);

        let target_height = match &self.state {
            LauncherState::RunningFull { .. } | LauncherState::FullOutput { .. } => {
                t.window.height.resolve(screen_h)
            }
            LauncherState::RofiMode { .. } => self.rofi_fit_height(screen_h),
            LauncherState::Search
            | LauncherState::ArgumentInput { .. }
            | LauncherState::Confirming { .. } => {
                // Argument chips wrap onto extra rows in narrow bars, so the
                // bar — and the window that fits to it — may need to be
                // taller than its nominal height.
                let ib_h = if let LauncherState::ArgumentInput {
                    target,
                    args,
                    values,
                    ..
                } = &self.state
                {
                    self.estimate_argument_bar_height(screen_w, target, args, values)
                } else {
                    ib_height
                };
                search_fit_height(
                    &t,
                    &SearchFit {
                        show_search,
                        should_show_list,
                        auto_height,
                        require_input,
                        ib_h,
                        columns,
                        filtered_len: self.filtered.len(),
                        element_font_size: self.element_font_val.1,
                        screen_h,
                    },
                )
            }
        };

        let win_width = if matches!(&self.state, LauncherState::RofiMode { .. }) {
            self.rofi_fit_width(screen_w)
        } else {
            t.window.width.resolve(screen_w).min(screen_w)
        };

        // Only forward a resize (and the re-center it triggers) when the
        // size actually changed: GPUI's macOS `resize` unconditionally calls
        // `setContentSize_`, and re-centering moves the window via
        // `setFrameOrigin` — doing either on every frame makes the native
        // window stutter during render bursts (e.g. while a script launches).
        let size_changed = match self.last_window_size {
            Some((w, h)) => (w - win_width).abs() > 0.5 || (h - target_height).abs() > 0.5,
            None => true,
        };
        if size_changed {
            window.resize(size(px(win_width), px(target_height)));
            self.last_window_size = Some((win_width, target_height));
            // Force exactly one follow-up frame to paint against the updated
            // native viewport size. Without this, GPUI leaves the uncovered
            // region stale if the app is otherwise idle.
            cx.spawn(|view: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let _ = view.update(&mut cx, |_, cx| cx.notify());
                }
            })
            .detach();
        }
        if self.needs_center || size_changed {
            self.needs_center = false;
            // In `require_input` search states the window height tracks the
            // result count, so centring it would make the search bar bob up
            // and down on every keystroke. Anchor the top edge to the
            // centred position of the nominal height instead, so the search
            // bar stays exactly where it is in the non-require_input theme
            // and the window grows/shrinks downward.
            let is_search_state = matches!(
                &self.state,
                LauncherState::Search
                    | LauncherState::ArgumentInput { .. }
                    | LauncherState::Confirming { .. }
            );
            let anchor_height = if (auto_height || require_input) && is_search_state {
                Some(t.window.height.resolve(screen_h) as f64)
            } else {
                None
            };
            crate::sys::appkit::center_window(
                t.window.x_offset as f64,
                t.window.y_offset as f64,
                anchor_height,
            );
        }
        // Reveal the window once a size transition (search ↔ Rofi) has fully
        // landed: `pending_reveal` was set when the window was dropped to
        // alpha 0 and a `resize_window_deferred` queued. The deferred resize
        // updates GPUI's `viewport_size` (the root size every frame is laid
        // out and painted with) only when it runs *between* App updates — so
        // wait until the viewport actually matches the target before
        // restoring opacity. Revealing earlier would show this frame painted
        // at the stale viewport size: stale content in the uncovered region
        // ("half the window doesn't render"). The frame counter is a safety
        // net so the window can never be left invisible.
        if self.pending_reveal {
            let vp = window.viewport_size();
            // Native window sizes snap to the device-pixel grid (an integer
            // number of pixels at any scale), so compare in device pixels
            // with a half-pixel tolerance instead of a fixed point value.
            let sf = window.scale_factor();
            let caught_up = (vp.width.as_f32() - win_width).abs() * sf <= 0.5
                && (vp.height.as_f32() - target_height).abs() * sf <= 0.5;
            if caught_up || self.reveal_wait_frames >= 8 {
                self.pending_reveal = false;
                self.reveal_wait_frames = 0;
                // Cancel the safety-net timer: the window is revealed now,
                // and a stray timer firing later would be a redundant
                // (if harmless) no-op — invalidate for cleanliness.
                crate::sys::appkit::invalidate_reveal_safety_net();
                crate::sys::appkit::set_window_alpha(1.0);
            } else {
                self.reveal_wait_frames += 1;
            }
        }

        // Keep the native blur layer in sync with the active theme: GPUI only
        // sets the background appearance at window creation, so it otherwise
        // lingers after the theme switcher / Cmd+R changes `[window] blur`.
        let desired_blur = t.window.blur;
        if self.last_blur != Some(desired_blur) {
            window.set_background_appearance(if desired_blur {
                WindowBackgroundAppearance::Blurred
            } else {
                WindowBackgroundAppearance::Transparent
            });
            self.last_blur = Some(desired_blur);
        }

        // Inner content: the actual launcher widgets or full page views.
        let inner = if let LauncherState::FullOutput { title } = &self.state {
            self.render_full_output(cx, title)
        } else if let LauncherState::RunningFull { title } = &self.state {
            self.render_full_output_running(title)
        } else if matches!(&self.state, LauncherState::RofiMode { .. }) {
            self.render_rofi_mode(cx)
        } else {
            let is_vertical = t.mainbox.orientation == "vertical";
            let mainbox_gap = t.mainbox.gap.unwrap_or(t.listview.spacing);
            let mut inner_box = div().flex_1().flex().gap(px(mainbox_gap));

            if is_vertical {
                inner_box = inner_box.flex_col();
            } else {
                inner_box = inner_box.flex_row();
            }

            for widget in &t.mainbox.children {
                match widget {
                    Widget::Builtin(BuiltinWidget::InputBar) if !show_search => {}
                    Widget::Builtin(BuiltinWidget::InputBar) => {
                        inner_box = inner_box.child(self.render_inputbar(cx));
                    }
                    Widget::Builtin(BuiltinWidget::ListView) => match &self.state {
                        LauncherState::Search => {
                            if should_show_list {
                                inner_box = inner_box.child(self.render_listview(cx, columns));
                            }
                        }
                        LauncherState::Confirming { target, .. } => {
                            inner_box = inner_box.child(self.render_confirmation(target, cx));
                        }
                        LauncherState::ArgumentInput { .. } => {
                            if let Some(options) = self.render_argument_options(cx) {
                                inner_box = inner_box.child(options);
                            }
                        }
                        _ => {}
                    },
                    Widget::Custom(id) => {
                        if let Some(element) = self.render_custom_widget(id, cx) {
                            inner_box = inner_box.child(element);
                        }
                    }
                }
            }
            inner_box.into_any()
        };

        // Wrap with background colour, padding, and optional background image.
        //
        // Size the root explicitly to the target window size rather than
        // `size_full()`. GPUI captures `root_size` from `viewport_size` at the
        // top of the draw pass, *before* `render()` runs — so on the first
        // frame after a resize the viewport is still the previous size and a
        // `size_full()` root would lay out (and clip) to that stale size. An
        // explicit size makes the layout independent of the viewport; the
        // native window is resized to match before it is shown.
        let opacity = t.window.background_opacity.unwrap_or(1.0);
        let mut root = div()
            .w(px(win_width))
            .h(px(target_height))
            .flex()
            .flex_col()
            .bg(rgba(Self::color(&t.window.background)))
            .text_color(rgba(Self::color(&t.element.text_color)))
            .font(self.root_font.clone())
            .text_size(px(t.font.size))
            .rounded(px(t.window.corner_radius))
            .overflow_hidden();

        if t.window.border_width > 0.0 {
            root = root
                .border(px(t.window.border_width))
                .border_color(rgba(Self::color(&t.window.border_color)));
        }

        if opacity < 1.0 {
            // Apply alpha to the root background colour.
            let hex = parse_hex_color_alpha(&t.window.background).unwrap_or(0x000000FF);
            let alpha = (opacity * 255.0) as u32;
            root = root.bg(rgba((hex & 0xFFFFFF00) | alpha));
        }

        // Full-page views (full output, running) need breathing room even in
        // zero-padding split layouts, so pad them up to the default window
        // padding plus the theme's `[script_view]` padding.
        let is_full_page = matches!(
            &self.state,
            LauncherState::FullOutput { .. } | LauncherState::RunningFull { .. }
        );
        let content_padding = if is_full_page {
            (t.window.padding + t.script_view.padding.unwrap_or(0.0)).max(16.0)
        } else {
            t.window.padding
        };
        let content = div()
            .flex_1()
            .flex()
            .flex_col()
            .p(px(content_padding))
            .child(inner);

        match t.window.background_image.as_deref() {
            Some(bg_path) => {
                let resolved = expand_tilde_path(bg_path);
                root = root.child(
                    img(std::path::PathBuf::from(&resolved))
                        .absolute()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover),
                );
                root = root.child(content);
            }
            None => {
                root = root.child(content);
            }
        }

        root
    }
}

/// State-dependent inputs for [`search_fit_height`]. The resolved booleans
/// (not the raw theme fields) are passed in so this stays a pure function of
/// "what is on screen right now".
pub(super) struct SearchFit {
    /// Whether the input bar is rendered. A script's `show_search false`
    /// metatag removes it — together with the mainbox gap that followed it.
    pub show_search: bool,
    /// Whether the results list is rendered this frame.
    pub should_show_list: bool,
    /// Resolved auto-height: fit the window to the results list.
    pub auto_height: bool,
    /// Resolved `listview.require_input`.
    pub require_input: bool,
    /// Input bar height (already wrapped-chip-adjusted for `ArgumentInput`).
    pub ib_h: f32,
    /// Effective list columns (1 = list mode, >1 = grid mode).
    pub columns: usize,
    /// Number of visible items (`filtered.len()`).
    pub filtered_len: usize,
    /// Element font size (`element_font_val.1`), for text-line metrics.
    pub element_font_size: f32,
    /// Screen height the window `Dimension` resolves against.
    pub screen_h: f32,
}

/// Window height for the `Search` / `ArgumentInput` / `Confirming` states.
///
/// Pure function of the theme and current state (no `&self`, no GPUI) so the
/// sizing math can be unit-tested. The list/grid heights mirror the exact
/// row and cell heights `render_row` / `render_grid_cell` lay out:
/// - list row: `2·pad + max(icon, text_h) + 2·border`
/// - grid cell: `2·pad + icon + icon_gap + text_h + 2·border`, packed into
///   `len.div_ceil(columns)` rows
///
/// Dropping the text-line/border terms under-sized the window and clipped
/// the last row; counting items instead of grid rows pinned the window to
/// its full height in grid themes.
pub(super) fn search_fit_height(t: &ThemeConfig, state: &SearchFit) -> f32 {
    let window_h = t.window.height.resolve(state.screen_h);
    if !(state.require_input || state.auto_height) {
        return window_h;
    }

    let pad_v = t.window.padding;
    let margin_bottom = t.inputbar.margin.get(2).copied().unwrap_or(8.0);
    let mainbox_gap = t.mainbox.gap.unwrap_or(t.listview.spacing);
    let spacing = t.listview.spacing;
    let el = &t.element;
    let el_pad_v = el.padding.first().copied().unwrap_or(8.0);
    // Same text-line metrics as `render_row` / `render_grid_cell`.
    let list_text_h = state.element_font_size * 1.2;
    let grid_text_h = (state.element_font_size - 1.5).max(11.0) * 1.2;

    if !state.should_show_list {
        // Compact: the bar only (its bottom margin is not counted — the bar
        // sits centred in the padding), or just the window padding when the
        // bar is hidden as well.
        let bar_h = if state.show_search { state.ib_h } else { 0.0 };
        return bar_h + pad_v * 2.0;
    }

    if state.auto_height {
        let (rows, row_h) = if state.columns > 1 {
            (
                state.filtered_len.div_ceil(state.columns),
                el_pad_v * 2.0 + el.icon_size + el.icon_gap + grid_text_h + el.border_width * 2.0,
            )
        } else {
            (
                state.filtered_len,
                el_pad_v * 2.0 + el.icon_size.max(list_text_h) + el.border_width * 2.0,
            )
        };
        let list_h = rows as f32 * row_h + (rows.saturating_sub(1) as f32) * spacing;
        // The bar's bottom margin and the mainbox gap between bar and list
        // exist only while the bar is rendered.
        let bar_block = if state.show_search {
            state.ib_h + margin_bottom + mainbox_gap
        } else {
            0.0
        };
        return (bar_block + list_h + pad_v * 2.0).min(window_h);
    }

    // `require_input` without auto-height: expand straight to the configured
    // height, but never below the bar (which wrapped argument chips can make
    // taller than nominal).
    let bar_block = if state.show_search {
        state.ib_h + margin_bottom
    } else {
        0.0
    };
    window_h.max(bar_block + pad_v * 2.0)
}

impl Launcher {
    /// Convenience: resolve a theme hex colour string to a `u32` for
    /// GPUI's `rgb()`, falling back to black on bad input.
    fn color(hex: &str) -> u32 {
        parse_hex_color_alpha(hex).unwrap_or(0x000000FF)
    }

    /// Current Rofi-mode name (from `@aerofi.preset`), if in Rofi mode.
    fn rofi_layout(&self) -> Option<&str> {
        match &self.state {
            LauncherState::RofiMode { layout, .. } => layout.as_deref(),
            _ => None,
        }
    }

    /// Effective column count for Rofi mode: the script's runtime `\0columns`
    /// (which inherits the `@aerofi.columns` metatag) wins, otherwise the
    /// preset's `columns`, otherwise 1.
    fn rofi_effective_columns(&self) -> usize {
        if self.rofi_columns() > 1 {
            self.rofi_columns()
        } else {
            self.rofi_layout()
                .and_then(|name| self.theme.presets.get(name))
                .and_then(|m| m.element.columns)
                .unwrap_or(1)
                .max(1)
        }
    }

    /// Calculate the width for Rofi mode, driven entirely by the theme or preset.
    pub(super) fn rofi_fit_width(&self, screen_w: f32) -> f32 {
        let t = &self.theme;
        self.rofi_layout()
            .and_then(|name| t.presets.get(name))
            .and_then(|p| p.window_width)
            .unwrap_or(t.window.width)
            .resolve(screen_w)
            .min(screen_w)
    }

    /// Rough height estimate for the argument input bar, used to size the
    /// window in `require_input` themes. Argument chips wrap onto extra rows
    /// when they don't fit, so the bar can be taller than its nominal height.
    ///
    /// The estimate assumes a monospaced font (advance ≈ 0.6em), which holds
    /// for the bundled themes; it is deliberately conservative and the result
    /// is always capped by the theme window height upstream, while the bar
    /// itself uses `min_h` so it can never be clipped.
    fn estimate_argument_bar_height(
        &self,
        screen_w: f32,
        target: &Target,
        args: &[crate::core::item::ScriptArgument],
        values: &[String],
    ) -> f32 {
        let t = &self.theme;
        let ib = &t.inputbar;
        let font_sz = self.inputbar_font.1;
        let char_w = font_sz * 0.6;

        let pad_v = ib.padding.first().copied().unwrap_or(10.0);
        let pad_h = ib.padding.get(1).copied().unwrap_or(14.0);
        let win_w = t.window.width.resolve(screen_w).min(screen_w);
        // Bar inner width minus the icon box, gaps, and window/bar padding.
        let icon_w = (ib.height - pad_v * 2.0).max(0.0);
        let bar_inner_w = (win_w - t.window.padding * 2.0 - pad_h * 2.0 - icon_w - 24.0).max(80.0);

        let text_w = |s: &str| s.chars().count() as f32 * char_w;
        let name_w = text_w(target.name());
        let chips_w: f32 = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let text = values
                    .get(i)
                    .map(String::as_str)
                    .filter(|v| !v.is_empty())
                    .or(a.placeholder.as_deref())
                    .unwrap_or("...");
                text_w(text) + 20.0 // chip horizontal padding + border
            })
            .sum();
        let gap_w = (args.len() as f32 + 1.0) * 8.0; // gap between items
        let line_h = font_sz * 1.2 + 8.0; // one text line + chip vertical padding

        let lines = ((name_w + chips_w + gap_w) / bar_inner_w).ceil().max(1.0);
        ib.height.max(lines * line_h + pad_v * 2.0)
    }

    /// Ideal window height for Rofi mode: fit the actual content (input bar,
    /// message banner, and the list/grid) instead of always using the full
    /// theme window height, so short menus (e.g. a power menu) don't leave a
    /// large empty area below the content. Capped at the theme window height
    /// so long lists stay scrollable.
    pub(super) fn rofi_fit_height(&self, screen_h: f32) -> f32 {
        let t = &self.theme;
        let pad_v = t.window.padding;
        let script_view_padding = t.script_view.padding.unwrap_or(0.0);
        let spacing = t.listview.spacing;

        let show_search = self
            .sticky_metatags
            .as_ref()
            .and_then(|m| m.show_search)
            .unwrap_or(true);

        let LauncherState::RofiMode {
            filtered_rows,
            message,
            ..
        } = &self.state
        else {
            return t.window.height.resolve(screen_h);
        };

        // Icon / padding metrics shared by list and grid rows.
        let el = &t.element;
        let mode_el = self
            .rofi_layout()
            .and_then(|name| t.presets.get(name))
            .map(|m| &m.element);
        let icon_size = mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size);
        let pad_v_el = mode_el
            .and_then(|m| m.padding.as_deref().and_then(|p| p.first().copied()))
            .unwrap_or_else(|| el.padding.first().copied().unwrap_or(8.0));
        let border = el.border_width;
        let text_h = ((t.font.size - 1.5).max(11.0)) * 1.2;

        // Stack the container's children, each separated by `spacing`.
        let mut blocks: Vec<f32> = Vec::new();
        if show_search {
            let ib_mb = t.inputbar.margin.get(2).copied().unwrap_or(8.0);
            blocks.push(t.inputbar.height + ib_mb);
        }
        if message.is_some() {
            blocks.push(t.font.size * 0.85 + 8.0);
        }

        let n = filtered_rows.len().max(1);
        let cols = self.rofi_effective_columns();
        let list_h = if cols > 1 {
            let rows = n.div_ceil(cols);
            let cell_h = pad_v_el * 2.0 + icon_size + el.icon_gap + text_h + border * 2.0;
            rows as f32 * cell_h + (rows.saturating_sub(1)) as f32 * spacing
        } else {
            let row_h = pad_v_el * 2.0 + icon_size.max(text_h) + border * 2.0;
            n as f32 * row_h + (n.saturating_sub(1)) as f32 * spacing
        };
        blocks.push(list_h);

        let content: f32 = blocks.iter().sum::<f32>() + spacing * (blocks.len() - 1) as f32;
        (content + pad_v * 2.0 + script_view_padding * 2.0 + 6.0)
            .min(t.window.height.resolve(screen_h))
    }

    /// Render the input bar styled from `theme.inputbar`.
    pub(super) fn render_inputbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let ib = &t.inputbar;

        let inner_view = if let LauncherState::ArgumentInput {
            target,
            args,
            values,
            focused_index,
        } = &self.state
        {
            // Chips wrap onto extra rows instead of being squeezed when the
            // bar is too narrow (compact themes, split layouts); the bar
            // itself grows via `min_h` below.
            let mut row = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .min_w(px(0.0))
                        .line_clamp(1)
                        .text_ellipsis()
                        .overflow_hidden()
                        .text_color(rgba(Self::color(&t.element.text_color)))
                        .child(target.name().to_string()),
                );

            for (i, arg) in args.iter().enumerate() {
                let is_focused = i == *focused_index;
                // Use accent for the focused chip's background and border so
                // the selection is always visible — even when the theme sets
                // `selected.background = transparent` (e.g. gruvbox).
                let bg_color = if is_focused {
                    rgba(Self::color(&t.status_colors.accent))
                } else {
                    rgba(0x00000000)
                };
                let border_color = if is_focused {
                    rgba(Self::color(&t.status_colors.accent))
                } else {
                    rgba(Self::color(&ib.placeholder_color))
                };
                let text_val = &values[i];
                let display_text = if text_val.is_empty() {
                    arg.placeholder.as_deref().unwrap_or("...").to_string()
                } else if arg.arg_type.as_deref() == Some("dropdown") {
                    if let Some(data) = &arg.data {
                        data.iter()
                            .find(|o| o.value == *text_val)
                            .map(|o| o.title.clone())
                            .unwrap_or_else(|| text_val.clone())
                    } else {
                        text_val.clone()
                    }
                } else {
                    text_val.clone()
                };
                let t_color = if is_focused {
                    rgba(Self::color(&t.element.selected.text_color))
                } else if text_val.is_empty() {
                    rgba(Self::color(&ib.placeholder_color))
                } else {
                    rgba(Self::color(&ib.text_color))
                };
                let chip = div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(bg_color)
                    .border_1()
                    .border_color(border_color)
                    .text_color(t_color)
                    .cursor(CursorStyle::PointingHand)
                    .flex_shrink_0()
                    .id(format!("arg-chip-{i}"))
                    .on_click(cx.listener(move |this, event, _window, cx| {
                        if is_primary_click(event) {
                            this.focus_argument(i);
                            cx.notify();
                        }
                    }))
                    .child(display_text);
                row = row.child(chip);
            }
            row.into_any()
        } else {
            let (ib_font, ib_size) = &self.inputbar_font;
            if self.query.is_empty() {
                div()
                    .flex_1()
                    .font(ib_font.clone())
                    .text_size(px(*ib_size))
                    .text_color(rgba(Self::color(&ib.placeholder_color)))
                    .child(ib.placeholder.clone())
                    .into_any()
            } else {
                div()
                    .flex_1()
                    .font(ib_font.clone())
                    .text_size(px(*ib_size))
                    .text_color(rgba(Self::color(&ib.text_color)))
                    .child(self.query.clone())
                    .into_any()
            }
        };

        let icon_label = ib.icon.as_deref().unwrap_or("❯");
        let icon_color = ib.icon_color.as_deref().unwrap_or(&ib.text_color);

        let padding_v = ib.padding.first().copied().unwrap_or(10.0);
        let padding_h = ib.padding.get(1).copied().unwrap_or(14.0);
        let margin_bottom = ib.margin.get(2).copied().unwrap_or(8.0);

        // Argument chips wrap onto multiple rows in narrow bars, so the bar
        // may need to be taller than its nominal height. In every other
        // state the bar is exactly `ib.height`.
        let is_argument_state = matches!(&self.state, LauncherState::ArgumentInput { .. });
        let bar = div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .px(px(padding_h))
            .py(px(padding_v))
            .mb(px(margin_bottom))
            .bg(rgba(Self::color(&ib.background)))
            .rounded(px(ib.corner_radius))
            .border(px(ib.border_width))
            .border_color(rgba(Self::color(&ib.border_color)));
        let bar = if is_argument_state {
            bar.min_h(px(ib.height))
        } else {
            bar.h(px(ib.height))
        };

        bar.child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(ib.height - padding_v * 2.0))
                .h(px(ib.height - padding_v * 2.0))
                .text_size(px(ib.height * 0.4))
                .text_color(rgba(Self::color(icon_color)))
                .child(icon_label.to_string()),
        )
        .child(inner_view)
        .into_any()
    }

    /// Render a virtualized options list for the focused argument when it is
    /// a dropdown with choices. Returns `None` when the focused argument is
    /// not a non-empty dropdown (nothing to show in the list area).
    pub(super) fn render_argument_options(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let LauncherState::ArgumentInput {
            args,
            focused_index,
            ..
        } = &self.state
        else {
            return None;
        };
        let arg = args.get(*focused_index)?;
        if arg.arg_type.as_deref() != Some("dropdown") {
            return None;
        }
        let options = arg.data.as_deref().filter(|d| !d.is_empty())?;

        // Each option is one text line with fixed vertical padding, so rows
        // are uniform and `uniform_list` can virtualize them — a script may
        // provide many choices and the list re-renders every frame.
        let option_h = 2.0 * 6.0 + self.theme.font.size * 1.2;
        let list = uniform_list(
            "arg-options",
            options.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                // Read the live state through `this` (the entity) so no
                // option data is cloned per frame.
                let (data, selected_val) = match &this.state {
                    LauncherState::ArgumentInput {
                        args,
                        values,
                        focused_index,
                        ..
                    } => (
                        args.get(*focused_index)
                            .and_then(|a| a.data.as_deref())
                            .unwrap_or(&[]),
                        values.get(*focused_index).map(String::as_str).unwrap_or(""),
                    ),
                    _ => (&[] as &[crate::core::item::ScriptArgumentOption], ""),
                };
                let t = &this.theme;
                range
                    .filter(|ix| *ix < data.len())
                    .map(|ix| {
                        let option = &data[ix];
                        let is_selected = option.value == selected_val;
                        // Use accent for the selected option's background so
                        // the selection is always visible — even when the theme
                        // sets `selected.background = transparent` (e.g. gruvbox).
                        let bg_color = if is_selected {
                            rgba(Self::color(&t.status_colors.accent))
                        } else {
                            rgba(0x00000000)
                        };
                        let text_color = if is_selected {
                            rgba(Self::color(&t.element.selected.text_color))
                        } else {
                            rgba(Self::color(&t.element.text_color))
                        };
                        let border_color = if is_selected {
                            rgba(Self::color(&t.status_colors.accent))
                        } else {
                            rgba(0x00000000)
                        };
                        div()
                            .w_full()
                            .h(px(option_h))
                            .px(px(12.0))
                            .rounded_md()
                            .bg(bg_color)
                            .border(px(1.0))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .text_color(text_color)
                            .child(option.title.clone())
                            .into_any()
                    })
                    .collect()
            }),
        );
        Some(
            div()
                .flex_1()
                .min_h(px(0.0))
                .w_full()
                .overflow_hidden()
                .px(px(8.0))
                .py(px(4.0))
                .child(list.h_full().w_full())
                .into_any(),
        )
    }

    /// Render the interactive Rofi-mode view: input bar with custom prompt,
    /// optional status message, and a scrollable list of script-provided rows.
    fn render_rofi_mode(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let ib = &t.inputbar;
        let el = &t.element;

        let LauncherState::RofiMode {
            title,
            rows,
            filtered_rows,
            prompt,
            message,
            selected,
            query,
            loading,
            active_indices,
            preview_blocks,
            multi_select,
            toggled_indices,
            markup_rows,
            ..
        } = &self.state
        else {
            return div().into_any();
        };

        let script_view_padding = t.script_view.padding.unwrap_or(0.0);
        let mut container = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(t.listview.spacing))
            .p(px(script_view_padding));

        // ── Input bar with optional prompt override and loading badge ───
        let placeholder = prompt.as_deref().unwrap_or(title.as_str());
        let inner_view = if query.is_empty() {
            div()
                .flex_1()
                .text_color(rgba(Self::color(&ib.placeholder_color)))
                .child(placeholder.to_string())
                .into_any()
        } else {
            div()
                .flex_1()
                .text_color(rgba(Self::color(&ib.text_color)))
                .child(query.clone())
                .into_any()
        };

        let icon_label = ib.icon.as_deref().unwrap_or("❯");
        let icon_color = ib.icon_color.as_deref().unwrap_or(&ib.text_color);
        let padding_v = ib.padding.first().copied().unwrap_or(10.0);
        let padding_h = ib.padding.get(1).copied().unwrap_or(14.0);
        let margin_bottom = ib.margin.get(2).copied().unwrap_or(8.0);

        let is_image = is_image_path(icon_label);

        let icon_el = if is_image {
            let resolved = crate::ui::launcher::helpers::expand_tilde_path(icon_label);
            img(std::path::PathBuf::from(resolved))
                .w(px(ib.height * 0.4))
                .h(px(ib.height * 0.4))
                .into_any()
        } else {
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(ib.height - padding_v * 2.0))
                .h(px(ib.height - padding_v * 2.0))
                .text_size(px(ib.height * 0.4))
                .text_color(rgba(Self::color(icon_color)))
                .child(icon_label.to_string())
                .into_any()
        };

        let mut inputbar = div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(px(ib.height))
            .px(px(padding_h))
            .py(px(padding_v))
            .mb(px(margin_bottom))
            .bg(rgba(Self::color(&ib.background)))
            .rounded(px(ib.corner_radius))
            .child(icon_el)
            .child(inner_view);

        if *loading {
            let loading_col = rgba(Self::color(icon_color));
            inputbar = inputbar.child(
                div()
                    .px_2()
                    .py(px(2.0))
                    .rounded_sm()
                    .border_1()
                    .border_color(loading_col)
                    .text_color(loading_col)
                    .text_size(px(t.font.size * 0.75))
                    .child("Loading…"),
            );
        }

        let show_search = self
            .sticky_metatags
            .as_ref()
            .and_then(|m| m.show_search)
            .unwrap_or(true);

        if show_search {
            container = container.child(inputbar);
        }

        // ── Optional message banner ─────────────────────────────────
        if let Some(msg) = message {
            let msg_el = div()
                .w_full()
                .px_2()
                .py_1()
                .text_color(rgba(Self::color(
                    el.description_color.as_deref().unwrap_or(&el.text_color),
                )))
                .text_size(px(t.font.size * 0.85))
                .child(msg.clone());
            container = container.child(msg_el);
        }

        // ── Layout (List + Optional Preview) ───────────────────────────
        let mut list_container = div().flex_1().flex().flex_col();

        // While the script is still loading, don't show the "no results"
        // placeholder — the "Loading…" message banner is the only feedback.
        if filtered_rows.is_empty() && !*loading {
            list_container = list_container.child(
                div()
                    .flex_1()
                    .px_2()
                    .py_1()
                    .text_color(rgba(Self::color(&t.listview.empty_text_color)))
                    .child(t.listview.empty_text.clone()),
            );
        } else if !filtered_rows.is_empty() {
            let mode_el = self
                .rofi_layout()
                .and_then(|name| t.presets.get(name))
                .map(|m| &m.element);
            let pad_v_el = mode_el
                .and_then(|m| m.padding.as_deref().and_then(|p| p.first().copied()))
                .unwrap_or_else(|| el.padding.first().copied().unwrap_or(8.0));
            let pad_h = mode_el
                .and_then(|m| m.padding.as_deref().and_then(|p| p.get(1).copied()))
                .unwrap_or_else(|| el.padding.get(1).copied().unwrap_or(12.0));
            let icon_size = px(mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size));
            let rofi_radius = mode_el
                .and_then(|m| m.corner_radius)
                .unwrap_or(el.corner_radius);
            let desc_color = rgba(Self::color(
                el.description_color.as_deref().unwrap_or(&el.text_color),
            ));
            // Explicit uniform row height for the single-column list (the
            // rows are virtualized with `uniform_list`, which only measures
            // the first row — see `render_row`).
            let icon_size_f32 = mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size);
            let rofi_row_h =
                pad_v_el * 2.0 + icon_size_f32.max(t.font.size * 1.2) + el.border_width * 2.0;

            let cols = self.rofi_effective_columns();
            if cols > 1 {
                // Grid mode: virtualized rows of `cols` cells each.
                let total_rows = filtered_rows.len().div_ceil(cols);
                let list = uniform_list(
                    "rofi_rows",
                    total_rows,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|row_ix| this.render_rofi_grid_row(row_ix, cols, _cx))
                            .collect()
                    }),
                )
                .track_scroll(&self.rofi_rows_scroll)
                .flex_1()
                .w_full();
                list_container = list_container.child(list);
            } else {
                let filtered_clone = filtered_rows.clone();
                let rows_clone = rows.clone();
                let active_indices_clone = active_indices.clone();
                let selected_val = *selected;
                let multi_select_val = *multi_select;
                let toggled_indices_clone = toggled_indices.clone();
                let markup_rows_val = *markup_rows;

                let list = uniform_list(
                    "rofi_rows",
                    filtered_clone.len(),
                    cx.processor(move |_this, range: std::ops::Range<usize>, _window, _cx| {
                        let t = &_this.theme;
                        let el = &t.element;
                        range
                            .map(|vis_ix| {
                                let row_idx = filtered_clone[vis_ix];
                                let row = &rows_clone[row_idx];
                                let is_selected = vis_ix == selected_val;
                                let is_active =
                                    row.active || active_indices_clone.contains(&row_idx);
                                let is_urgent = row.urgent;
                                let is_disabled = row.disabled;
                                let is_selectable = !row.nonselectable && !is_disabled;
                                let is_toggled = toggled_indices_clone.contains(&row_idx);

                                // Shrink padding by border width on the selected
                                // row so total row height stays constant.
                                let effective_pad_v = if is_selected && el.border_width > 0.0 {
                                    (pad_v_el - el.border_width).max(0.0)
                                } else {
                                    pad_v_el
                                };

                                let (row_bg, name_color) = if is_selected {
                                    (
                                        rgba(Self::color(&el.selected.background)),
                                        rgba(Self::color(&el.selected.text_color)),
                                    )
                                } else if is_toggled {
                                    (
                                        rgba(Self::color(&el.selected.background)).opacity(0.4),
                                        rgba(Self::color(&el.selected.text_color)),
                                    )
                                } else if !is_selectable {
                                    (rgba(0x00000000), desc_color)
                                } else if is_urgent {
                                    (
                                        rgba(Self::color(&t.status_colors.urgent_row_background)),
                                        rgba(Self::color(&el.text_color)),
                                    )
                                } else if is_active {
                                    (
                                        rgba(Self::color(&t.status_colors.active_row_background)),
                                        rgba(Self::color(&el.text_color)),
                                    )
                                } else {
                                    (rgba(0x00000000), rgba(Self::color(&el.text_color)))
                                };

                                if is_selectable {
                                    let id = format!("rofi-row-{vis_ix}");
                                    let mut row_div = div()
                                        .id(id)
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .w_full()
                                        .h(px(rofi_row_h))
                                        .px(px(pad_h))
                                        .py(px(effective_pad_v))
                                        .rounded(px(rofi_radius))
                                        .bg(row_bg)
                                        .cursor(CursorStyle::PointingHand);

                                    if is_selected {
                                        row_div = row_div.border(px(el.border_width)).border_color(
                                            rgba(Self::color(&t.status_colors.accent)),
                                        );
                                    } else if el.background != "transparent" {
                                        row_div = row_div.border(px(el.border_width)).border_color(
                                            rgba(Self::color(&t.window.border_color)),
                                        );
                                    }

                                    if multi_select_val {
                                        let toggle_icon = if is_toggled { "☑" } else { "☐" };
                                        row_div = row_div.child(
                                            div()
                                                .text_color(if is_toggled {
                                                    rgba(Self::color(
                                                        &t.status_colors.active_background,
                                                    ))
                                                } else {
                                                    desc_color
                                                })
                                                .text_size(px(t.font.size))
                                                .child(toggle_icon),
                                        );
                                    }

                                    if el.show_icons {
                                        row_div = row_div.child(Self::render_rofi_row_icon(
                                            icon_size, &row.icon, name_color,
                                        ));
                                    }

                                    let text_div = div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .line_clamp(1)
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .text_color(name_color);
                                    let text_div = if markup_rows_val {
                                        text_div.child(_this.markup_text(&row.text))
                                    } else if let Some(st) =
                                        _this.query_highlighted_text(&row.text, None)
                                    {
                                        text_div.child(st)
                                    } else {
                                        text_div.child(row.text.clone())
                                    };
                                    row_div = row_div.child(text_div);

                                    if is_urgent {
                                        row_div = row_div.child(
                                            div()
                                                .px_2()
                                                .py(px(2.0))
                                                .rounded_sm()
                                                .bg(rgba(Self::color(
                                                    &t.status_colors.urgent_background,
                                                )))
                                                .text_color(rgba(Self::color(
                                                    &t.status_colors.urgent_text,
                                                )))
                                                .text_size(px(t.font.size * 0.72))
                                                .child("URGENT"),
                                        );
                                    }

                                    if is_active {
                                        row_div = row_div.child(
                                            div()
                                                .px_2()
                                                .py(px(2.0))
                                                .rounded_sm()
                                                .bg(rgba(Self::color(
                                                    &t.status_colors.active_background,
                                                )))
                                                .text_color(rgba(Self::color(
                                                    &t.status_colors.active_text,
                                                )))
                                                .text_size(px(t.font.size * 0.72))
                                                .child("ACTIVE"),
                                        );
                                    }

                                    if let Some(info) = &row.info {
                                        let info_color = rgba(Self::color(
                                            el.description_color
                                                .as_deref()
                                                .unwrap_or(&el.text_color),
                                        ));
                                        let info_el = div()
                                            .text_color(info_color)
                                            .text_size(px(t.font.size * 0.82))
                                            .flex_shrink_0();
                                        let info_el = if markup_rows_val {
                                            info_el.child(_this.markup_text(info))
                                        } else {
                                            info_el.child(info.clone())
                                        };
                                        row_div = row_div.child(info_el);
                                    }

                                    row_div
                                        .on_click(_cx.listener(move |this, event, _window, cx| {
                                            if is_primary_click(event) {
                                                if let LauncherState::RofiMode {
                                                    selected, ..
                                                } = &mut this.state
                                                {
                                                    *selected = vis_ix;
                                                }
                                                this.rofi_select_row(cx);
                                                cx.notify();
                                            }
                                        }))
                                        .into_any()
                                } else {
                                    let mut row_div = div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .w_full()
                                        .h(px(rofi_row_h))
                                        .px(px(pad_h))
                                        .py(px(pad_v_el))
                                        .rounded(px(rofi_radius))
                                        .bg(row_bg);

                                    if is_disabled {
                                        row_div = row_div.opacity(0.4);
                                    }

                                    if multi_select_val {
                                        let toggle_icon = if is_toggled { "☑" } else { "☐" };
                                        row_div = row_div.child(
                                            div()
                                                .text_color(if is_toggled {
                                                    rgba(Self::color(
                                                        &t.status_colors.active_background,
                                                    ))
                                                } else {
                                                    desc_color
                                                })
                                                .text_size(px(t.font.size))
                                                .child(toggle_icon),
                                        );
                                    }

                                    if el.show_icons {
                                        row_div = row_div.child(Self::render_rofi_row_icon(
                                            icon_size, &row.icon, name_color,
                                        ));
                                    }

                                    let text_div = div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .line_clamp(1)
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .text_color(name_color);
                                    let text_div = if markup_rows_val {
                                        text_div.child(_this.markup_text(&row.text))
                                    } else if let Some(st) =
                                        _this.query_highlighted_text(&row.text, None)
                                    {
                                        text_div.child(st)
                                    } else {
                                        text_div.child(row.text.clone())
                                    };
                                    row_div = row_div.child(text_div);

                                    if is_urgent {
                                        row_div = row_div.child(
                                            div()
                                                .px_2()
                                                .py(px(2.0))
                                                .rounded_sm()
                                                .bg(rgba(Self::color(
                                                    &t.status_colors.urgent_background,
                                                )))
                                                .text_color(rgba(Self::color(
                                                    &t.status_colors.urgent_text,
                                                )))
                                                .text_size(px(t.font.size * 0.72))
                                                .child("URGENT"),
                                        );
                                    }

                                    if is_active {
                                        row_div = row_div.child(
                                            div()
                                                .px_2()
                                                .py(px(2.0))
                                                .rounded_sm()
                                                .bg(rgba(Self::color(
                                                    &t.status_colors.active_background,
                                                )))
                                                .text_color(rgba(Self::color(
                                                    &t.status_colors.active_text,
                                                )))
                                                .text_size(px(t.font.size * 0.72))
                                                .child("ACTIVE"),
                                        );
                                    }

                                    if let Some(info) = &row.info {
                                        let info_color = rgba(Self::color(
                                            el.description_color
                                                .as_deref()
                                                .unwrap_or(&el.text_color),
                                        ));
                                        let info_el = div()
                                            .text_color(info_color)
                                            .text_size(px(t.font.size * 0.82))
                                            .flex_shrink_0();
                                        let info_el = if markup_rows_val {
                                            info_el.child(_this.markup_text(info))
                                        } else {
                                            info_el.child(info.clone())
                                        };
                                        row_div = row_div.child(info_el);
                                    }

                                    row_div.into_any()
                                }
                            })
                            .collect()
                    }),
                )
                .flex_1()
                .w_full()
                .track_scroll(&self.rofi_rows_scroll);

                list_container = list_container.child(list);
            }
        }

        if preview_blocks.is_some() {
            let preview_panel = gpui::list(
                self.preview_list.clone(),
                cx.processor(move |this: &mut Launcher, ix: usize, _window, _cx| {
                    if let LauncherState::RofiMode {
                        preview_blocks: Some(b),
                        preview_styled,
                        ..
                    } = &this.state
                    {
                        let styled = preview_styled
                            .as_ref()
                            .and_then(|v| v.get(ix))
                            .and_then(|s| s.as_ref());
                        b.get(ix)
                            .map(|blk| this.render_md_block(blk, styled))
                            .unwrap_or_else(|| div().into_any())
                    } else {
                        div().into_any()
                    }
                }),
            )
            .flex_1()
            .w_full()
            .pl_3();

            container = container.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .w_full()
                    .child(list_container.w_1_2())
                    .child(
                        div()
                            .w_1_2()
                            .h_full()
                            .border_l_1()
                            .border_color(rgba(Self::color(&t.window.border_color)))
                            .child(preview_panel),
                    ),
            );
        } else {
            container = container.child(list_container.w_full());
        }

        container.into_any()
    }

    /// Render the icon element for a Rofi-mode row. Glyph icons use the
    /// row's foreground colour so they stay readable on selection.
    fn render_rofi_row_icon(
        icon_size: gpui::Pixels,
        icon: &Option<String>,
        icon_color: gpui::Rgba,
    ) -> gpui::AnyElement {
        if let Some(icon_str) = icon {
            // Strip the "emoji:" prefix used by scripts to hint the type
            let display = icon_str.strip_prefix("emoji:").unwrap_or(icon_str.as_str());
            let is_image = is_image_path(display);
            if is_image {
                let resolved = expand_tilde_path(display);
                img(std::path::PathBuf::from(resolved))
                    .w(icon_size)
                    .h(icon_size)
                    .rounded_sm()
                    .into_any()
            } else {
                // Emoji/symbol glyphs have line boxes ~1.17em tall (Apple Color
                // Emoji metrics), so render at 0.85× the box size to keep the
                // text inside the fixed-size icon box without overflowing.
                div()
                    .w(icon_size)
                    .h(icon_size)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(icon_size * 0.85)
                    .text_color(icon_color)
                    .child(display.to_string())
                    .into_any()
            }
        } else {
            div()
                .w(icon_size)
                .h(icon_size)
                .flex()
                .items_center()
                .justify_center()
                .text_size(icon_size * 0.85)
                .text_color(icon_color)
                .child("•".to_string())
                .into_any()
        }
    }

    /// Render one row of Rofi-mode grid cells (used when columns > 1).
    fn render_rofi_grid_row(
        &self,
        row_ix: usize,
        cols: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let spacing = px(t.listview.spacing);
        let total = if let LauncherState::RofiMode { filtered_rows, .. } = &self.state {
            filtered_rows.len()
        } else {
            return div().into_any();
        };
        let start_ix = row_ix * cols;
        let end_ix = (start_ix + cols).min(total);
        let total_rows = total.div_ceil(cols);

        let mut row = div().flex().gap(spacing).w_full();
        if row_ix + 1 < total_rows {
            row = row.pb(spacing);
        }
        for vis_ix in start_ix..end_ix {
            row = row.child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.render_rofi_grid_cell(vis_ix, cx)),
            );
        }
        // Pad incomplete last row to keep column alignment.
        for _ in end_ix..(start_ix + cols) {
            row = row.child(div().flex_1());
        }
        row.into_any()
    }

    /// Render a single Rofi-mode grid cell: icon on top, row text below.
    fn render_rofi_grid_cell(&self, vis_ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let el = &t.element;
        let mode_el = self
            .rofi_layout()
            .and_then(|name| t.presets.get(name))
            .map(|m| &m.element);
        let icon_size = px(mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size));
        let pad_v = mode_el
            .and_then(|m| m.padding.as_deref().and_then(|p| p.first().copied()))
            .unwrap_or_else(|| el.padding.first().copied().unwrap_or(8.0));
        let pad_h = mode_el
            .and_then(|m| m.padding.as_deref().and_then(|p| p.get(1).copied()))
            .unwrap_or_else(|| el.padding.get(1).copied().unwrap_or(12.0));
        let rofi_radius = mode_el
            .and_then(|m| m.corner_radius)
            .unwrap_or(el.corner_radius);

        let (row, is_selected, is_active, is_urgent, is_disabled, is_toggled, markup_rows) =
            if let LauncherState::RofiMode {
                rows,
                filtered_rows,
                selected,
                active_indices,
                toggled_indices,
                markup_rows,
                ..
            } = &self.state
            {
                let Some(&row_idx) = filtered_rows.get(vis_ix) else {
                    return div().into_any();
                };
                let Some(row) = rows.get(row_idx) else {
                    return div().into_any();
                };
                (
                    row,
                    vis_ix == *selected,
                    row.active || active_indices.contains(&row_idx),
                    row.urgent,
                    row.disabled,
                    toggled_indices.contains(&row_idx),
                    *markup_rows,
                )
            } else {
                return div().into_any();
            };

        let is_selectable = !row.nonselectable && !is_disabled;

        let (row_bg, name_color) = if is_selected {
            (
                rgba(Self::color(&el.selected.background)),
                rgba(Self::color(&el.selected.text_color)),
            )
        } else if is_toggled {
            (
                rgba(Self::color(&el.selected.background)).opacity(0.4),
                rgba(Self::color(&el.selected.text_color)),
            )
        } else if !is_selectable {
            (
                rgba(0x00000000),
                rgba(Self::color(
                    el.description_color.as_deref().unwrap_or(&el.text_color),
                )),
            )
        } else if is_urgent {
            (
                rgba(Self::color(&t.status_colors.urgent_row_background)),
                rgba(Self::color(&el.text_color)),
            )
        } else if is_active {
            (
                rgba(Self::color(&t.status_colors.active_row_background)),
                rgba(Self::color(&el.text_color)),
            )
        } else {
            (rgba(0x00000000), rgba(Self::color(&el.text_color)))
        };

        // Shrink padding by border width on the selected cell so the total
        // cell height stays constant.
        let effective_pad_v = if is_selected && el.border_width > 0.0 {
            (pad_v - el.border_width).max(0.0)
        } else {
            pad_v
        };

        // Explicit uniform cell height (see `render_grid_cell`).
        let icon_size_f32 = mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size);
        let text_h = ((t.font.size - 1.5).max(11.0)) * 1.2;
        let cell_h = pad_v * 2.0 + icon_size_f32 + el.icon_gap + text_h + el.border_width * 2.0;

        let mut cell = div()
            .id(format!("rofi-cell-{vis_ix}"))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(el.icon_gap))
            .w_full()
            .h(px(cell_h))
            .px(px(pad_h))
            .py(px(effective_pad_v))
            .rounded(px(rofi_radius))
            .bg(row_bg);

        if is_selected {
            cell = cell
                .border(px(el.border_width))
                .border_color(rgba(Self::color(&t.status_colors.accent)));
        } else if el.background != "transparent" {
            cell = cell
                .border(px(el.border_width))
                .border_color(rgba(Self::color(&t.window.border_color)));
        }

        if is_selectable {
            cell = cell.cursor(CursorStyle::PointingHand);
        }
        if is_disabled {
            cell = cell.opacity(0.4);
        }

        if el.show_icons {
            cell = cell.child(Self::render_rofi_row_icon(icon_size, &row.icon, name_color));
        }

        if !row.text.is_empty() {
            let text_el = div()
                .w_full()
                .flex()
                .justify_center()
                .text_color(name_color)
                .text_size(px((t.font.size - 1.5).max(11.0)))
                .line_clamp(1)
                .overflow_hidden();
            let text_el = if markup_rows {
                text_el.child(self.markup_text(&row.text))
            } else if let Some(st) = self.query_highlighted_text(&row.text, None) {
                text_el.child(st)
            } else {
                text_el.child(row.text.clone())
            };
            cell = cell.child(text_el);
        }

        if is_selectable {
            cell = cell.on_click(cx.listener(move |this, event, _window, cx| {
                if is_primary_click(event) {
                    if let LauncherState::RofiMode { selected, .. } = &mut this.state {
                        *selected = vis_ix;
                    }
                    this.rofi_select_row(cx);
                    cx.notify();
                }
            }));
        }

        cell.into_any()
    }

    /// Render the `LauncherState::Confirming` view when a dangerous action requires confirmation.
    pub(super) fn render_confirmation(
        &self,
        target: &Target,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let text_color = rgba(Self::color(&t.element.text_color));
        let sel_bg = rgba(Self::color(&t.element.selected.background));

        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .text_size(px(t.font.size * 1.2))
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
                                    this.perform_action(action, false, cx);
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
        let sel_bg = rgba(Self::color(&t.element.selected.background));
        let sel_text = rgba(Self::color(&t.element.selected.text_color));

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
                    .text_size(px(t.font.size * 0.8))
                    .text_color(sel_text)
                    .child("▶ Running"),
            )
            .child(
                div()
                    .text_size(px(t.font.size))
                    .text_color(rgba(Self::color(&t.element.text_color)))
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
            .border_color(rgba(Self::color(&t.window.border_color)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_color(rgba(Self::color(&t.status_colors.accent)))
                            .text_size(px(t.font.size * 0.8))
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
                            .text_size(px(t.font.size))
                            .text_color(rgba(Self::color(&t.element.text_color)))
                            .child(title.to_string()),
                    ),
            )
            .child(
                div()
                    .text_size(px(t.font.size * 0.7))
                    .text_color(rgba(Self::color(&t.status_colors.muted)))
                    .child("↵ Rerun"),
            );

        let block_count = self.full_output_blocks.len();
        let body = if block_count == 0 {
            div()
                .flex_1()
                .text_size(px(t.font.size * 0.8))
                .font_family(&self.mono_font)
                .text_color(rgba(Self::color(&t.inputbar.placeholder_color)))
                .child("(no output)")
                .into_any()
        } else {
            gpui::list(
                self.full_output_list.clone(),
                cx.processor(move |this: &mut Launcher, ix: usize, _window, _cx| {
                    let styled = this.full_output_styled.get(ix).and_then(|s| s.as_ref());
                    this.full_output_blocks
                        .get(ix)
                        .map(|b| this.render_md_block(b, styled))
                        .unwrap_or_else(|| div().into_any())
                }),
            )
            .flex_1()
            .w_full()
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
    fn render_md_block(
        &self,
        block: &crate::core::markdown::MdBlock,
        styled: Option<&MdStyled>,
    ) -> gpui::AnyElement {
        use crate::core::markdown::{MdBlock, TableAlignment};
        let t = &self.theme;
        let text_color = rgba(Self::color(&t.element.text_color));
        let dim_color = rgba(Self::color(&t.inputbar.placeholder_color));
        let mono = self.mono_font.clone();

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
                    .child(self.styled_md_text(text, styled))
                    .into_any()
            }
            MdBlock::Paragraph(text) => apply_md_style(div().w_full(), &base)
                .child(self.styled_md_text(text, styled))
                .into_any(),
            MdBlock::Blockquote(text) => {
                let mut style = base.clone();
                style.color = Self::hsla(&t.inputbar.placeholder_color);
                apply_md_style(
                    div()
                        .w_full()
                        .border_l_2()
                        .border_color(rgba(Self::color(&t.window.border_color)))
                        .pl_3(),
                    &style,
                )
                .child(self.styled_md_text(text, styled))
                .into_any()
            }
            MdBlock::CodeBlock { lang, text } => {
                let mut box_ = div()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(rgba(Self::color(&t.window.border_color)))
                    .bg(rgba(Self::color(&t.inputbar.background)))
                    .p_3();
                if let Some(lang) = lang {
                    box_ = box_.child(
                        div()
                            .text_size(px(t.font.size * 0.7))
                            .text_color(dim_color)
                            .mb_1()
                            .child(lang.clone()),
                    );
                }
                box_ = box_.child(
                    div()
                        .font_family(mono)
                        .text_size(px(t.font.size * 0.85))
                        .text_color(text_color)
                        .child(text.clone()),
                );
                box_.into_any()
            }
            MdBlock::Table {
                alignments,
                cells,
                col_weights,
                ..
            } => {
                // Real table: flex rows with theme borders. Column widths are
                // proportional to each column's widest cell (flex-grow
                // weights, precomputed at parse time), so columns stay
                // aligned across rows and shrink (wrapping cell text) in
                // narrow windows.
                let border_color = rgba(Self::color(&t.window.border_color));
                let align_div = |cell: gpui::Div, c: usize| -> gpui::Div {
                    match alignments.get(c).copied() {
                        Some(TableAlignment::Center) => cell.text_center(),
                        Some(TableAlignment::Right) => cell.text_right(),
                        _ => cell.text_left(),
                    }
                };
                let render_cells = |cells: &[String], bold: bool| -> gpui::Div {
                    let mut row = div().w_full().flex().flex_row();
                    for (c, w) in col_weights.iter().enumerate() {
                        // SmolStr-backed: no allocation for short cells.
                        let cell = cells.get(c).map_or_else(gpui::SharedString::default, |s| {
                            gpui::SharedString::from(s.as_str())
                        });
                        let mut cell_div = div()
                            .flex_1()
                            .flex_grow(*w as f32)
                            .min_w(px(0.0))
                            .px_2()
                            .py_1()
                            .font_family(&mono)
                            .text_size(px(t.font.size * 0.8))
                            .text_color(text_color)
                            .child(cell);
                        if bold {
                            cell_div = cell_div.font_weight(gpui::FontWeight::BOLD);
                        }
                        row = row.child(align_div(cell_div, c));
                    }
                    row
                };
                let mut table = div()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .bg(rgba(Self::color(&t.window.background)));
                let header_row = cells.first().map(Vec::as_slice).unwrap_or_default();
                table = table.child(
                    render_cells(header_row, true)
                        .border_b_1()
                        .border_color(border_color),
                );
                for (i, row) in cells.iter().skip(1).enumerate() {
                    let mut r = render_cells(row, false);
                    if i + 1 < cells.len() - 1 {
                        r = r.border_b_1().border_color(border_color);
                    }
                    table = table.child(r);
                }
                table.into_any()
            }
            MdBlock::ListItem { number, text } => {
                let marker = number.map_or_else(|| "•".to_string(), |n| format!("{n}."));
                apply_md_style(div().w_full().flex().flex_row().gap_2(), &base)
                    .child(div().text_color(text_color).child(marker))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(self.styled_md_text(text, styled)),
                    )
                    .into_any()
            }
            MdBlock::Rule => div()
                .w_full()
                .h(px(1.0))
                .my_1()
                .bg(rgba(Self::color(&t.window.border_color)))
                .into_any(),
            MdBlock::Plain(text) => div()
                .w_full()
                .font_family(mono)
                .text_size(px(t.font.size * 0.85))
                .text_color(dim_color)
                .child(text.clone())
                .into_any(),
        }
    }

    /// Precompute the GPUI styling for a markdown block (None for blocks
    /// without inline text). Runs when the block is parsed and when the
    /// theme is reloaded, not on every render pass.
    pub(super) fn build_md_styled(
        &self,
        block: &crate::core::markdown::MdBlock,
    ) -> Option<MdStyled> {
        use crate::core::markdown::MdBlock;
        let md = match block {
            MdBlock::Heading { text, .. } | MdBlock::ListItem { text, .. } => text,
            MdBlock::Paragraph(md) | MdBlock::Blockquote(md) => md,
            _ => return None,
        };
        Some(self.styled_for_md(md))
    }

    /// Walk a text block's inline marks into highlight ranges (with
    /// theme-derived styles) and code-span ranges.
    fn styled_for_md(&self, md: &crate::core::markdown::MdText) -> MdStyled {
        use crate::core::markdown::InlineKind;
        let t = &self.theme;

        let mut highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> =
            Vec::with_capacity(md.marks.len());
        let mut code_ranges: Vec<std::ops::Range<usize>> = Vec::new();
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
                    color: Some(Self::hsla_hex(Self::color(&t.status_colors.accent))),
                    underline: Some(gpui::UnderlineStyle::default()),
                    ..Default::default()
                },
                InlineKind::Code => gpui::HighlightStyle {
                    background_color: Some(
                        Self::hsla_hex(Self::color(&t.status_colors.muted)).opacity(0.35),
                    ),
                    ..Default::default()
                },
            };
            if mark.kind == InlineKind::Code {
                code_ranges.push(mark.range.clone());
            }
            highlights.push((mark.range.clone(), style));
        }
        MdStyled {
            highlights,
            code_ranges,
        }
    }

    /// Build a GPUI `StyledText` element for markdown text: one text layout
    /// per block with per-range highlights for inline emphasis and a
    /// monospace font override for inline code. The base style (family,
    /// size, colour) is inherited from the parent element's `text_style`.
    fn styled_md_text(
        &self,
        md: &crate::core::markdown::MdText,
        styled: Option<&MdStyled>,
    ) -> gpui::StyledText {
        // Clone the two small precomputed vecs instead of re-walking the
        // marks; fall back to computing them when a parallel entry is
        // missing (defensive: blocks built outside the usual paths).
        let (highlights, code_ranges) = match styled {
            Some(s) => (s.highlights.clone(), s.code_ranges.clone()),
            None => {
                let s = self.styled_for_md(md);
                (s.highlights, s.code_ranges)
            }
        };

        let mut styled = gpui::StyledText::new(md.text.clone());
        if !code_ranges.is_empty() {
            styled = styled.with_font_family_overrides(
                code_ranges.into_iter().map(|r| (r, self.mono_font.clone())),
            );
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
    fn hsla_hex(rgba: u32) -> gpui::Hsla {
        gpui::Rgba {
            r: ((rgba >> 24) & 0xff) as f32 / 255.0,
            g: ((rgba >> 16) & 0xff) as f32 / 255.0,
            b: ((rgba >> 8) & 0xff) as f32 / 255.0,
            a: (rgba & 0xff) as f32 / 255.0,
        }
        .into()
    }

    /// Render the result list styled from `theme.listview` and `theme.element`.
    /// When `columns > 1`, items are laid out in a grid.
    pub(super) fn render_listview(
        &self,
        cx: &mut Context<Self>,
        columns: usize,
    ) -> impl IntoElement {
        let t = &self.theme;

        if self.filtered.is_empty() {
            return div()
                .flex_1()
                .px_2()
                .py_1()
                .text_color(rgba(Self::color(&t.listview.empty_text_color)))
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
        let total_rows = self.filtered.len().div_ceil(cols);

        // No bottom padding on the last row: it would add dead space below
        // the list and can push content past the viewport, which makes
        // scroll_to_item(Nearest) snap when selecting rows below.
        let mut row = div().flex().gap(spacing).w_full();
        if row_ix + 1 < total_rows {
            row = row.pb(spacing);
        }
        for global_ix in start_ix..end_ix {
            let item = &self.all[self.filtered[global_ix]];
            let is_selected = global_ix == self.selected;
            row = row.child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.render_grid_cell(item, is_selected, global_ix, cx)),
            );
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
                rgba(Self::color(&el.selected.background)),
                rgba(Self::color(&el.selected.text_color)),
            )
        } else if el.background != "transparent" {
            (
                rgba(Self::color(&el.background)),
                rgba(Self::color(&el.text_color)),
            )
        } else {
            (rgba(0x00000000), rgba(Self::color(&el.text_color)))
        };

        // Glyph icons normally take the inputbar accent colour, but on a
        // selected cell they must follow the row foreground (a theme may use
        // the accent for both, which would make the icon invisible).
        let icon_color = if is_selected {
            name_color
        } else {
            rgba(Self::color(
                t.inputbar
                    .icon_color
                    .as_deref()
                    .unwrap_or(&t.inputbar.text_color),
            ))
        };

        let icon_size = px(el.icon_size);
        let icon_element = if el.show_icons {
            let inner = if let Some(path) = item.icon_path().or(item.icon_image_path()) {
                img(path)
                    .w(icon_size)
                    .h(icon_size)
                    .rounded(px(el.icon_radius))
                    .into_any()
            } else {
                let fallback = item.icon().unwrap_or("•");
                let is_image = is_image_path(fallback);
                if is_image {
                    let p = expand_tilde_path(fallback);
                    img(std::path::PathBuf::from(p))
                        .w(icon_size)
                        .h(icon_size)
                        .rounded(px(el.icon_radius))
                        .into_any()
                } else {
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(icon_size)
                        .h(icon_size)
                        .text_size(icon_size * 0.85)
                        .text_color(icon_color)
                        .child(fallback.to_string())
                        .into_any()
                }
            };
            // Fixed-size wrapper ensures uniform cell height regardless of icon type.
            div()
                .w(icon_size)
                .h(icon_size)
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(inner)
                .into_any()
        } else {
            div().into_any()
        };

        // `[element].padding` is `[vertical, horizontal]`.
        let pad_v = el.padding.first().copied().unwrap_or(8.0);
        let pad_h = el.padding.get(1).copied().unwrap_or(12.0);
        // Shrink padding by border width on the selected cell so the total
        // cell height stays constant.
        let effective_pad_v = if is_selected && el.border_width > 0.0 {
            (pad_v - el.border_width).max(0.0)
        } else {
            pad_v
        };

        // Explicit uniform cell height: grid rows are virtualized with
        // `uniform_list`, which measures only the first row. Emoji-preset
        // names fall back to the emoji font, whose line metrics differ from
        // the theme font, so without a fixed height the icon columns would
        // drift row by row.
        let text_h = (self.element_font_val.1 - 1.5).max(11.0) * 1.2;
        let cell_h = pad_v * 2.0 + el.icon_size + el.icon_gap + text_h + el.border_width * 2.0;

        let mut cell_div = div()
            .w_full()
            .h(px(cell_h))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(el.icon_gap))
            .py(px(effective_pad_v))
            .px(px(pad_h))
            .rounded(px(el.corner_radius))
            .cursor(CursorStyle::PointingHand)
            .bg(cell_bg)
            .overflow_hidden();

        if is_selected {
            cell_div = cell_div
                .border(px(el.border_width))
                .border_color(rgba(Self::color(&t.status_colors.accent)));
        } else if el.background != "transparent" {
            cell_div = cell_div
                .border(px(el.border_width))
                .border_color(rgba(Self::color(&t.window.border_color)));
        }

        let cell_name = item.name().to_string();
        let cell_name_el: gpui::AnyElement = match self.query_highlighted_text(&cell_name, None) {
            Some(st) => st.into_any(),
            None => cell_name.into_any_element(),
        };
        let (cell_font, cell_size) = &self.element_font_val;
        Self::with_item_mouse_handlers(
            cell_div.child(icon_element).child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .font(cell_font.clone())
                    .text_color(name_color)
                    .text_size(px((*cell_size - 1.5).max(11.0)))
                    .line_clamp(1)
                    .overflow_hidden()
                    .child(cell_name_el),
            ),
            format!("cell-{filtered_ix}"),
            filtered_ix,
            cx,
        )
        .into_any()
    }

    /// Build a single list row for `filtered_ix` (position within `filtered`).
    fn render_row(&self, filtered_ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let item = &self.all[self.filtered[filtered_ix]];
        let is_selected = filtered_ix == self.selected;
        let t = &self.theme;
        let el = &t.element;

        let (row_bg, name_color) = if is_selected {
            (
                rgba(Self::color(&el.selected.background)),
                rgba(Self::color(&el.selected.text_color)),
            )
        } else {
            (rgba(0x00000000), rgba(Self::color(&el.text_color)))
        };

        // See render_grid_cell: glyph icons follow the row foreground when
        // selected so they stay visible on the selection background.
        let icon_color = if is_selected {
            name_color
        } else {
            rgba(Self::color(
                t.inputbar
                    .icon_color
                    .as_deref()
                    .unwrap_or(&t.inputbar.text_color),
            ))
        };

        // `[element].padding` is `[vertical, horizontal]` — standard CSS order.
        let pad_v = el.padding.first().copied().unwrap_or(8.0);
        let pad_h = el.padding.get(1).copied().unwrap_or(12.0);

        // When a border is drawn, shrink padding by the same amount so the
        // total row height (content + padding + border) stays constant.
        let effective_pad_v = if is_selected && el.border_width > 0.0 {
            (pad_v - el.border_width).max(0.0)
        } else {
            pad_v
        };

        let desc_color = rgba(Self::color(
            el.description_color.as_deref().unwrap_or(&el.text_color),
        ));

        // Explicit uniform row height. `uniform_list` measures only the first
        // row and lays out every other row at multiples of that size, so any
        // per-row content-height variance (a wrapped name, an emoji font's
        // line metrics differing from the theme font) would shift every row
        // below it and the icon column would drift. Clamping the name to one
        // line plus a fixed height keeps all rows pixel-identical.
        let text_h = self.element_font_val.1 * 1.2;
        let row_h = pad_v * 2.0 + el.icon_size.max(text_h) + el.border_width * 2.0;

        let mut row = div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(px(row_h))
            .px(px(pad_h))
            .py(px(effective_pad_v))
            .rounded(px(el.corner_radius))
            .cursor(CursorStyle::PointingHand)
            .bg(row_bg);

        if is_selected {
            row = row
                .border(px(el.border_width))
                .border_color(rgba(Self::color(&t.status_colors.accent)));
        } else if el.background != "transparent" {
            row = row
                .border(px(el.border_width))
                .border_color(rgba(Self::color(&t.window.border_color)));
        }

        if let Some(layout) = &el.layout {
            for slot in layout {
                match slot.as_str() {
                    "icon" => {
                        row = row.child(self.render_row_icon(item, icon_color));
                    }
                    "name" => {
                        row = row.child(self.render_row_name(
                            item,
                            name_color,
                            desc_color,
                            is_selected,
                        ));
                    }
                    "spacer" | "flex" => {
                        row = row.child(div().flex_1());
                    }
                    "shortcuts" | "shortcut" => {
                        if let Some(sc) = self.render_shortcut_badge(item, desc_color) {
                            row = row.child(sc);
                        }
                    }
                    "badge" | "category" | "category_badge" => {
                        if t.listview.category_badge.show {
                            row = row.child(self.render_category_badge(item));
                        }
                    }
                    "alias_badge" => {
                        if let Some(badge) = self.render_alias_badge(item) {
                            row = row.child(badge);
                        }
                    }
                    custom_id => {
                        if let Some(widget_el) =
                            self.render_custom_widget_for_row(custom_id, filtered_ix, item, cx)
                        {
                            row = row.child(widget_el);
                        }
                    }
                }
            }
        } else {
            row = row
                .child(self.render_row_icon(item, icon_color))
                .child(self.render_row_name(item, name_color, desc_color, is_selected));
            // The name takes its natural width, so insert an explicit spacer
            // to push the trailing badges to the right edge.
            row = row.child(div().flex_1());
            if let Some(alias) = self.render_alias_badge(item) {
                row = row.child(alias);
            }
            if t.listview.category_badge.show {
                row = row.child(self.render_category_badge(item));
            }
            if let Some(sc) = self.render_shortcut_badge(item, desc_color) {
                row = row.child(sc);
            }
        }

        Self::with_item_mouse_handlers(row, format!("row-{filtered_ix}"), filtered_ix, cx)
            .into_any()
    }

    fn render_row_icon(&self, item: &Target, icon_color: gpui::Rgba) -> gpui::AnyElement {
        let el = &self.theme.element;
        let icon_size = px(el.icon_size);
        if el.show_icons {
            if let Some(path) = item.icon_path() {
                img(path)
                    .w(icon_size)
                    .h(icon_size)
                    .rounded(px(el.icon_radius))
                    .into_any()
            } else if let Some(icon_str) = item.icon() {
                let is_image = is_image_path(icon_str);

                if is_image {
                    // Scripts carry a pre-resolved icon path (computed at
                    // parse time); other targets fall back to resolving here.
                    let resolved_el = if let Some(pre) = item.icon_image_path() {
                        img(pre)
                            .w(icon_size)
                            .h(icon_size)
                            .rounded(px(el.icon_radius))
                    } else {
                        let resolved = if icon_str.starts_with('~') || icon_str.starts_with('/') {
                            std::path::PathBuf::from(expand_tilde_path(icon_str))
                        } else {
                            match item {
                                Target::Script { path, .. } => path
                                    .parent()
                                    .map(|d| d.join(icon_str))
                                    .unwrap_or_else(|| std::path::PathBuf::from(icon_str)),
                                _ => std::path::PathBuf::from(icon_str),
                            }
                        };
                        img(resolved)
                            .w(icon_size)
                            .h(icon_size)
                            .rounded(px(el.icon_radius))
                    };
                    resolved_el.into_any()
                } else {
                    div()
                        .w(icon_size)
                        .h(icon_size)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(icon_size * 0.85)
                        .text_color(icon_color)
                        .child(icon_str.to_string())
                        .into_any()
                }
            } else {
                div()
                    .w(icon_size)
                    .h(icon_size)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(icon_size * 0.85)
                    .text_color(icon_color)
                    .child("•".to_string())
                    .into_any()
            }
        } else {
            div().into_any()
        }
    }

    /// Byte ranges (with a highlight style) of the query's matched
    /// characters inside `name`, or `None` when highlighting is disabled,
    /// the query is empty, or the name doesn't match.
    fn name_highlights(
        &self,
        name: &str,
        selected_color: Option<gpui::Hsla>,
    ) -> Option<Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>> {
        let t = &self.theme;
        if !t.listview.highlight_matches || self.query.is_empty() || name.is_empty() {
            return None;
        }
        // The ranges depend only on (query, name); look them up in the
        // per-query memo so selection changes and redraws don't re-run the
        // fuzzy matcher for every visible row on every render pass.
        let ranges = {
            let mut cache = self.highlight_cache.borrow_mut();
            match cache.get(name) {
                Some(r) => r.clone(),
                None => {
                    let r = crate::core::search::highlight_ranges(
                        name,
                        &self.query,
                        self.app_config.general.matching,
                    );
                    cache.insert(gpui::SharedString::from(name), r.clone());
                    r
                }
            }
        };
        if ranges.is_empty() {
            return None;
        }
        let color = selected_color.unwrap_or_else(|| {
            Self::hsla_hex(Self::color(
                t.listview
                    .match_color
                    .as_deref()
                    .unwrap_or(&t.status_colors.accent),
            ))
        });
        Some(
            ranges
                .into_iter()
                .map(|r| {
                    (
                        r,
                        gpui::HighlightStyle {
                            color: Some(color),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        )
    }

    /// Pango-parsed `StyledText` for a markup Rofi row. The parse (a linear
    /// scan with per-tag allocations) is memoized per row text, so redraws
    /// and selection changes don't re-parse every visible row.
    fn markup_text(&self, text: &str) -> gpui::StyledText {
        let (plain, highlights) = {
            let mut cache = self.pango_cache.borrow_mut();
            match cache.get(text) {
                Some(c) => (c.0.clone(), c.1.clone()),
                None => {
                    let v = crate::core::pango::parse_pango(text);
                    cache.insert(gpui::SharedString::from(text), (v.0.clone(), v.1.clone()));
                    v
                }
            }
        };
        let mut st = gpui::StyledText::new(plain);
        if !highlights.is_empty() {
            st = st.with_highlights(highlights);
        }
        st
    }

    /// A `StyledText` with the query match highlighted, or `None` when there
    /// is nothing to highlight.
    fn query_highlighted_text(
        &self,
        text: &str,
        selected_color: Option<gpui::Hsla>,
    ) -> Option<gpui::StyledText> {
        let hl = self.name_highlights(text, selected_color)?;
        Some(gpui::StyledText::new(text).with_highlights(hl))
    }

    fn render_row_name(
        &self,
        item: &Target,
        name_color: gpui::Rgba,
        desc_color: gpui::Rgba,
        is_selected: bool,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let subtitle_opt = item.inline_output().or_else(|| item.package_name());

        let name = item.name();
        let sel_color = if is_selected {
            Some(Self::hsla_hex(Self::color(&t.element.selected.text_color)))
        } else {
            None
        };
        let name_el: gpui::AnyElement = match self.query_highlighted_text(name, sel_color) {
            Some(st) => st.into_any(),
            None => name.to_string().into_any_element(),
        };
        let (name_font, name_size) = &self.element_font_val;

        // The name takes its natural width (shrinking with an ellipsis only
        // when it truly overflows). It must not `flex_1()`: layouts like
        // ["icon", "name", "spacer"] would otherwise split the leftover width
        // 50/50 between name and spacer, truncating names mid-row.
        if let Some(subtitle) = subtitle_opt {
            div()
                .min_w(px(0.0))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .min_w(px(0.0))
                        .line_clamp(1)
                        .text_ellipsis()
                        .overflow_hidden()
                        .font(name_font.clone())
                        .text_size(px(*name_size))
                        .text_color(name_color)
                        .child(name_el),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(t.font.size - 2.0))
                        .text_color(desc_color)
                        .child(subtitle.to_string()),
                )
                .into_any()
        } else {
            div()
                .min_w(px(0.0))
                .line_clamp(1)
                .text_ellipsis()
                .overflow_hidden()
                .font(name_font.clone())
                .text_size(px(*name_size))
                .text_color(name_color)
                .child(name_el)
                .into_any()
        }
    }

    fn render_category_badge(&self, item: &Target) -> gpui::AnyElement {
        let t = &self.theme;
        let b = &t.listview.category_badge;
        let col = rgba(Self::color(&b.color));
        let font_sz = px((t.font.size - b.font_size_offset).max(8.0));
        let mut badge = div().text_color(col).text_size(font_sz);
        if b.padding_x > 0.0 {
            badge = badge.px(px(b.padding_x));
        }
        if b.radius > 0.0 {
            badge = badge.rounded(px(b.radius));
        }
        if b.border {
            let bc = b.border_color.as_deref().unwrap_or(&b.color);
            badge = badge.border(px(1.0)).border_color(rgba(Self::color(bc)));
        }
        badge.child(item.category_label().to_string()).into_any()
    }

    /// Render the alias pill badges for a target (all configured aliases that
    /// point at it). Returns `None` when the badge is disabled or the target
    /// has no aliases, so the layout slot renders nothing.
    fn render_alias_badge(&self, item: &Target) -> Option<gpui::AnyElement> {
        let t = &self.theme;
        let b = &t.listview.alias_badge;
        if !b.show {
            return None;
        }
        let aliases = self.alias_labels(item.name());
        if aliases.is_empty() {
            return None;
        }
        let col = rgba(Self::color(&b.color));
        let font_sz = px((t.font.size - b.font_size_offset).max(8.0));
        let mut pills = div().flex().items_center().gap_1();
        for alias in &aliases {
            let mut pill = div()
                .text_size(font_sz)
                .text_color(col)
                .child(alias.clone());
            if b.padding_x > 0.0 {
                pill = pill.px(px(b.padding_x));
            }
            if b.radius > 0.0 {
                pill = pill.rounded(px(b.radius));
            }
            if b.border {
                let bc = b.border_color.as_deref().unwrap_or(&b.color);
                pill = pill.border(px(1.0)).border_color(rgba(Self::color(bc)));
            }
            pills = pills.child(pill);
        }
        Some(pills.into_any())
    }

    /// All configured aliases that point at the given target's display name.
    pub(super) fn alias_labels(&self, name: &str) -> Vec<String> {
        self.app_config
            .aliases
            .iter()
            .filter(|(_, target)| target.as_str() == name)
            .map(|(alias, _)| alias.clone())
            .collect()
    }

    fn render_shortcut_badge(
        &self,
        item: &Target,
        desc_color: gpui::Rgba,
    ) -> Option<gpui::AnyElement> {
        let label = self.shortcut_label(item.name())?;
        let t = &self.theme;
        Some(
            div()
                .text_size(px(t.font.size - 2.0))
                .text_color(desc_color)
                .child(label)
                .into_any(),
        )
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
                    let action = this.execute_selected(false);
                    this.perform_action(action, false, cx);
                }
            }))
    }

    /// The shortcuts bound to a target, for display next to its row: the
    /// global combo first, then the launcher-local one, in macOS glyph form
    /// (e.g. `"⌥G  ⌘R"`). `None` when the target has no bound shortcut.
    pub(super) fn shortcut_label(&self, name: &str) -> Option<String> {
        let global = self
            .app_config
            .bindings
            .global
            .iter()
            .find(|(_, target)| target.as_str() == name)
            .map(|(combo, _)| combo.clone());
        let local = self
            .app_config
            .bindings
            .launcher
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

#[cfg(test)]
mod fit_tests {
    use super::{SearchFit, search_fit_height};
    use crate::core::theme::{Dimension, ThemeConfig};

    /// Deterministic theme: fixed 500pt window so `screen_h` is irrelevant.
    fn fit_theme() -> ThemeConfig {
        let mut t = ThemeConfig::default();
        t.window.height = Dimension::Points(500.0);
        t.window.padding = 10.0;
        t.inputbar.height = 40.0;
        t.inputbar.margin = vec![0.0, 0.0, 8.0, 0.0]; // bottom = 8
        t.mainbox.gap = Some(12.0);
        t.listview.spacing = 6.0;
        t.element.padding = vec![6.0, 10.0];
        t.element.icon_size = 24.0;
        t.element.icon_gap = 4.0;
        t.element.border_width = 0.0;
        t
    }

    fn fit(
        show_search: bool,
        show_list: bool,
        auto_height: bool,
        require_input: bool,
        columns: usize,
        n: usize,
        font: f32,
    ) -> SearchFit {
        SearchFit {
            show_search,
            should_show_list: show_list,
            auto_height,
            require_input,
            ib_h: 40.0,
            columns,
            filtered_len: n,
            element_font_size: font,
            screen_h: 1000.0,
        }
    }

    /// List mode, font (20.4) shorter than icon (24): row = 2·6 + 24 + 0 = 36.
    #[test]
    fn list_mode_fits_rows_exactly() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(true, true, true, true, 1, 3, 17.0));
        // bar(40) + margin(8) + gap(12) + [3·36 + 2·6] + pad(20) = 200
        assert!((h - 200.0).abs() < 1e-3, "got {h}");
    }

    /// Fix A: element borders were omitted from the row height, under-sizing
    /// the window and clipping the last row.
    #[test]
    fn list_mode_counts_element_borders() {
        let mut t = fit_theme();
        t.element.border_width = 1.0;
        let h = search_fit_height(&t, &fit(true, true, true, true, 1, 3, 17.0));
        // row = 2·6 + 24 + 2·1 = 38 → 40+8+12 + [3·38 + 2·6] + 20 = 206
        assert!((h - 206.0).abs() < 1e-3, "got {h}");
    }

    /// Fix A: a font taller than the icon drives the row height.
    #[test]
    fn list_mode_uses_taller_text_line() {
        let t = fit_theme();
        // font 30 → text_h 36 > icon 24 → row = 12 + 36 = 48
        let h = search_fit_height(&t, &fit(true, true, true, true, 1, 2, 30.0));
        // 40+8+12 + [2·48 + 6] + 20 = 182
        assert!((h - 182.0).abs() < 1e-3, "got {h}");
    }

    /// Fix B: grid themes pack `n.div_ceil(columns)` rows, not `n`.
    #[test]
    fn grid_mode_counts_rows_not_items() {
        let t = fit_theme();
        // cell = 2·6 + 24 + 4 + (17-1.5)·1.2 + 0 = 58.6; 8 items / 4 cols = 2 rows
        let h = search_fit_height(&t, &fit(true, true, true, true, 4, 8, 17.0));
        // 40+8+12 + [2·58.6 + 6] + 20 = 203.2
        assert!((h - 203.2).abs() < 1e-3, "got {h}");
    }

    /// Fix B: a partially-filled last row still occupies a full row.
    #[test]
    fn grid_mode_partial_last_row() {
        let t = fit_theme();
        // 10 items / 4 cols = 3 rows
        let h = search_fit_height(&t, &fit(true, true, true, true, 4, 10, 17.0));
        // 40+8+12 + [3·58.6 + 2·6] + 20 = 267.8
        assert!((h - 267.8).abs() < 1e-3, "got {h}");
    }

    /// Fix C: `show_search false` removes the bar, its margin and the gap.
    #[test]
    fn hidden_bar_drops_bar_and_gap() {
        let t = fit_theme();
        let with_bar = search_fit_height(&t, &fit(true, true, true, true, 1, 3, 17.0));
        let without_bar = search_fit_height(&t, &fit(false, true, true, true, 1, 3, 17.0));
        // bar(40) + margin(8) + gap(12) = 60 less
        assert!((with_bar - without_bar - 60.0).abs() < 1e-3);
        assert!((without_bar - 140.0).abs() < 1e-3, "got {without_bar}");
    }

    /// Compact (list hidden) with the bar: bar + padding only (no margin).
    #[test]
    fn compact_shows_bar_only() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(true, false, true, true, 1, 0, 17.0));
        assert!((h - 60.0).abs() < 1e-3, "got {h}");
    }

    /// Compact with the bar hidden too: only the window padding remains.
    #[test]
    fn compact_without_bar_is_padding_only() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(false, false, true, true, 1, 0, 17.0));
        assert!((h - 20.0).abs() < 1e-3, "got {h}");
    }

    /// Long lists cap at the configured window height.
    #[test]
    fn fits_are_capped_at_window_height() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(true, true, true, true, 1, 1000, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// require_input without auto-height: full height, never below the bar.
    #[test]
    fn require_input_without_auto_height_is_full() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(true, true, false, true, 1, 3, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// Neither flag: plain full height, no fitting at all.
    #[test]
    fn no_fitting_is_full_height() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(true, true, false, false, 1, 3, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// Decoupled case: auto-height on without require-input (list always shown).
    #[test]
    fn auto_height_without_require_input_fits() {
        let t = fit_theme();
        let h = search_fit_height(&t, &fit(true, true, true, false, 1, 3, 17.0));
        assert!((h - 200.0).abs() < 1e-3, "got {h}");
    }
}
