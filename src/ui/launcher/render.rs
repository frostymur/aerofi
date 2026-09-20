//! GPUI rendering for the launcher: the `Render` impl and all private
//! view-building helpers (inputbar, listview, grid, full-output, markdown).

use gpui::{
    Context, CursorStyle, Render, Window, WindowBackgroundAppearance, div, img, prelude::*, px,
    rgba, size, uniform_list,
};

use crate::core::item::Target;
use crate::core::theme::{BuiltinWidget, Widget, parse_hex_color_alpha};

use super::helpers::{
    apply_md_style, expand_tilde_path, format_combo, is_image_path, is_primary_click,
};
use super::state::Launcher;
use super::types::LauncherState;

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;

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
            LauncherState::GuiMode { .. } => self.gui_fit_height(),
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
        let win_width = if matches!(&self.state, LauncherState::GuiMode { .. }) {
            self.sticky_metatags
                .as_ref()
                .and_then(|m| m.width)
                .unwrap_or(t.window.width)
        } else {
            t.window.width
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
        }
        if self.needs_center || size_changed {
            self.needs_center = false;
            crate::sys::appkit::center_window(t.window.x_offset as f64, t.window.y_offset as f64);
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
        } else if matches!(&self.state, LauncherState::GuiMode { .. }) {
            self.render_gui_mode(cx)
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
                    Widget::Builtin(BuiltinWidget::ListView) => {
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
                    Widget::Builtin(BuiltinWidget::Banner) => {
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
        let opacity = t.window.background_opacity.unwrap_or(1.0);
        let mut root = div()
            .size_full()
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
        // padding plus the theme's `[gui]` padding.
        let is_full_page = matches!(
            &self.state,
            LauncherState::FullOutput { .. } | LauncherState::RunningFull { .. }
        );
        let content_padding = if is_full_page {
            (t.window.padding + t.gui.padding.unwrap_or(0.0)).max(16.0)
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
    /// Convenience: resolve a theme hex colour string to a `u32` for
    /// GPUI's `rgb()`, falling back to black on bad input.
    fn color(hex: &str) -> u32 {
        parse_hex_color_alpha(hex).unwrap_or(0x000000FF)
    }

    /// Current GUI-mode name (from `@aerofi.preset`), if in GUI mode.
    fn gui_layout(&self) -> Option<&str> {
        match &self.state {
            LauncherState::GuiMode { layout, .. } => layout.as_deref(),
            _ => None,
        }
    }

    /// Effective column count for GUI mode: the script's runtime `\0columns`
    /// (which inherits the `@aerofi.columns` metatag) wins, otherwise the
    /// preset's `columns`, otherwise 1.
    fn gui_effective_columns(&self) -> usize {
        if self.gui_columns() > 1 {
            self.gui_columns()
        } else {
            self.gui_layout()
                .and_then(|name| self.theme.presets.get(name))
                .and_then(|m| m.element.columns)
                .unwrap_or(1)
                .max(1)
        }
    }

    /// Ideal window height for GUI mode: fit the actual content (input bar,
    /// message banner, and the list/grid) instead of always using the full
    /// theme window height, so short menus (e.g. a power menu) don't leave a
    /// large empty area below the content. Capped at the theme window height
    /// so long lists stay scrollable.
    fn gui_fit_height(&self) -> f32 {
        let t = &self.theme;
        let pad_v = t.window.padding;
        let gui_padding = t.gui.padding.unwrap_or(0.0);
        let spacing = t.listview.spacing;

        let show_search = self
            .sticky_metatags
            .as_ref()
            .and_then(|m| m.show_search)
            .unwrap_or(true);

        let LauncherState::GuiMode {
            filtered_rows,
            message,
            ..
        } = &self.state
        else {
            return t.window.height;
        };

        // Icon / padding metrics shared by list and grid rows.
        let el = &t.element;
        let mode_el = self
            .gui_layout()
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
        let cols = self.gui_effective_columns();
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
        (content + pad_v * 2.0 + gui_padding * 2.0 + 6.0).min(t.window.height)
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
            let mut row = div().flex().flex_row().items_center().gap_2().child(
                div()
                    .text_color(rgba(Self::color(&t.element.text_color)))
                    .child(target.name().to_string()),
            );

            for (i, arg) in args.iter().enumerate() {
                let is_focused = i == *focused_index;
                let bg_color = if is_focused {
                    rgba(Self::color(&t.element.selected.background))
                } else {
                    rgba(0x00000000)
                };
                let border_color = if is_focused {
                    rgba(Self::color(&t.element.selected.background))
                } else {
                    rgba(Self::color(&ib.placeholder_color))
                };
                let text_val = &values[i];
                let display_text = if text_val.is_empty() {
                    arg.placeholder.as_deref().unwrap_or("...")
                } else {
                    text_val
                };
                let t_color = if text_val.is_empty() {
                    rgba(Self::color(&ib.placeholder_color))
                } else {
                    rgba(Self::color(&ib.text_color))
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
            .bg(rgba(Self::color(&ib.background)))
            .rounded(px(ib.corner_radius))
            .border(px(ib.border_width))
            .border_color(rgba(Self::color(&ib.border_color)))
            .child(
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

    /// Render the interactive GUI-mode view: input bar with custom prompt,
    /// optional status message, and a scrollable list of script-provided rows.
    fn render_gui_mode(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let ib = &t.inputbar;
        let el = &t.element;

        let LauncherState::GuiMode {
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

        let gui_padding = t.gui.padding.unwrap_or(0.0);
        let mut container = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(t.listview.spacing))
            .p(px(gui_padding));

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
        let padding_h = ib.padding.first().copied().unwrap_or(12.0);
        let padding_v = ib.padding.get(1).copied().unwrap_or(16.0);
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
                .gui_layout()
                .and_then(|name| t.presets.get(name))
                .map(|m| &m.element);
            let pad_h = mode_el
                .and_then(|m| m.padding.as_deref().and_then(|p| p.get(1).copied()))
                .unwrap_or_else(|| el.padding.get(1).copied().unwrap_or(12.0));
            let pad_v_el = mode_el
                .and_then(|m| m.padding.as_deref().and_then(|p| p.first().copied()))
                .unwrap_or_else(|| el.padding.first().copied().unwrap_or(8.0));
            let icon_size = px(mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size));
            let gui_radius = mode_el
                .and_then(|m| m.corner_radius)
                .unwrap_or(el.corner_radius);
            let desc_color = rgba(Self::color(
                el.description_color.as_deref().unwrap_or(&el.text_color),
            ));

            let cols = self.gui_effective_columns();
            if cols > 1 {
                // Grid mode: virtualized rows of `cols` cells each.
                let total_rows = filtered_rows.len().div_ceil(cols);
                let list = uniform_list(
                    "gui_rows",
                    total_rows,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|row_ix| this.render_gui_grid_row(row_ix, cols, _cx))
                            .collect()
                    }),
                )
                .track_scroll(&self.gui_rows_scroll)
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
                    "gui_rows",
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
                                    let id = format!("gui-row-{vis_ix}");
                                    let mut row_div = div()
                                        .id(id)
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .w_full()
                                        .px(px(pad_h))
                                        .py(px(effective_pad_v))
                                        .rounded(px(gui_radius))
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
                                        row_div = row_div.child(Self::render_gui_row_icon(
                                            icon_size, &row.icon, name_color,
                                        ));
                                    }

                                    let text_div = div().flex_1().text_color(name_color);
                                    let text_div = if markup_rows_val {
                                        let (plain, highlights) =
                                            crate::core::pango::parse_pango(&row.text);
                                        let mut st = gpui::StyledText::new(plain);
                                        if !highlights.is_empty() {
                                            st = st.with_highlights(highlights);
                                        }
                                        text_div.child(st)
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
                                            let (plain, highlights) =
                                                crate::core::pango::parse_pango(info);
                                            let mut st = gpui::StyledText::new(plain);
                                            if !highlights.is_empty() {
                                                st = st.with_highlights(highlights);
                                            }
                                            info_el.child(st)
                                        } else {
                                            info_el.child(info.clone())
                                        };
                                        row_div = row_div.child(info_el);
                                    }

                                    row_div
                                        .on_click(_cx.listener(move |this, event, _window, cx| {
                                            if is_primary_click(event) {
                                                if let LauncherState::GuiMode { selected, .. } =
                                                    &mut this.state
                                                {
                                                    *selected = vis_ix;
                                                }
                                                this.gui_select_row(cx);
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
                                        .px(px(pad_h))
                                        .py(px(pad_v_el))
                                        .rounded(px(gui_radius))
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
                                        row_div = row_div.child(Self::render_gui_row_icon(
                                            icon_size, &row.icon, name_color,
                                        ));
                                    }

                                    let text_div = div().flex_1().text_color(name_color);
                                    let text_div = if markup_rows_val {
                                        let (plain, highlights) =
                                            crate::core::pango::parse_pango(&row.text);
                                        let mut st = gpui::StyledText::new(plain);
                                        if !highlights.is_empty() {
                                            st = st.with_highlights(highlights);
                                        }
                                        text_div.child(st)
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
                                            let (plain, highlights) =
                                                crate::core::pango::parse_pango(info);
                                            let mut st = gpui::StyledText::new(plain);
                                            if !highlights.is_empty() {
                                                st = st.with_highlights(highlights);
                                            }
                                            info_el.child(st)
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
                .track_scroll(&self.gui_rows_scroll);

                list_container = list_container.child(list);
            }
        }

        if preview_blocks.is_some() {
            let preview_panel = gpui::list(
                self.preview_list.clone(),
                cx.processor(move |this: &mut Launcher, ix: usize, _window, _cx| {
                    if let LauncherState::GuiMode {
                        preview_blocks: Some(b),
                        ..
                    } = &this.state
                    {
                        b.get(ix)
                            .map(|blk| this.render_md_block(blk))
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

    /// Render the icon element for a GUI-mode row. Glyph icons use the
    /// row's foreground colour so they stay readable on selection.
    fn render_gui_row_icon(
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

    /// Render one row of GUI-mode grid cells (used when columns > 1).
    fn render_gui_grid_row(
        &self,
        row_ix: usize,
        cols: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let spacing = px(t.listview.spacing);
        let total = if let LauncherState::GuiMode { filtered_rows, .. } = &self.state {
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
                    .child(self.render_gui_grid_cell(vis_ix, cx)),
            );
        }
        // Pad incomplete last row to keep column alignment.
        for _ in end_ix..(start_ix + cols) {
            row = row.child(div().flex_1());
        }
        row.into_any()
    }

    /// Render a single GUI-mode grid cell: icon on top, row text below.
    fn render_gui_grid_cell(&self, vis_ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = &self.theme;
        let el = &t.element;
        let mode_el = self
            .gui_layout()
            .and_then(|name| t.presets.get(name))
            .map(|m| &m.element);
        let icon_size = px(mode_el.and_then(|m| m.icon_size).unwrap_or(el.icon_size));
        let pad_h = mode_el
            .and_then(|m| m.padding.as_deref().and_then(|p| p.get(1).copied()))
            .unwrap_or_else(|| el.padding.get(1).copied().unwrap_or(12.0));
        let pad_v = mode_el
            .and_then(|m| m.padding.as_deref().and_then(|p| p.first().copied()))
            .unwrap_or_else(|| el.padding.first().copied().unwrap_or(8.0));
        let gui_radius = mode_el
            .and_then(|m| m.corner_radius)
            .unwrap_or(el.corner_radius);

        let (row, is_selected, is_active, is_urgent, is_disabled, is_toggled, markup_rows) =
            if let LauncherState::GuiMode {
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

        let mut cell = div()
            .id(format!("gui-cell-{vis_ix}"))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(el.icon_gap))
            .w_full()
            .px(px(pad_h))
            .py(px(effective_pad_v))
            .rounded(px(gui_radius))
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
            cell = cell.child(Self::render_gui_row_icon(icon_size, &row.icon, name_color));
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
                let (plain, highlights) = crate::core::pango::parse_pango(&row.text);
                let mut st = gpui::StyledText::new(plain);
                if !highlights.is_empty() {
                    st = st.with_highlights(highlights);
                }
                text_el.child(st)
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
                    if let LauncherState::GuiMode { selected, .. } = &mut this.state {
                        *selected = vis_ix;
                    }
                    this.gui_select_row(cx);
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
                    this.full_output_blocks
                        .get(ix)
                        .map(|b| this.render_md_block(b))
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
    fn render_md_block(&self, block: &crate::core::markdown::MdBlock) -> gpui::AnyElement {
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
                        .border_color(rgba(Self::color(&t.window.border_color)))
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
                            .child(self.styled_md_text(text)),
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

    /// Build a GPUI `StyledText` element for markdown text: one text layout
    /// per block with per-range highlights for inline emphasis and a
    /// monospace font override for inline code. The base style (family,
    /// size, colour) is inherited from the parent element's `text_style`.
    fn styled_md_text(&self, md: &crate::core::markdown::MdText) -> gpui::StyledText {
        use crate::core::markdown::InlineKind;
        let t = &self.theme;

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
                code_ranges.push((mark.range.clone(), self.mono_font.clone()));
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

        let pad_v = el.padding.get(1).copied().unwrap_or(10.0);
        let pad_h = el.padding.first().copied().unwrap_or(8.0);
        // Shrink padding by border width on the selected cell so the total
        // cell height stays constant.
        let effective_pad_v = if is_selected && el.border_width > 0.0 {
            (pad_v - el.border_width).max(0.0)
        } else {
            pad_v
        };

        let mut cell_div = div()
            .w_full()
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

        let pad_h = el.padding.first().copied().unwrap_or(8.0);
        let pad_v = el.padding.get(1).copied().unwrap_or(12.0);

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

        let mut row = div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
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
                    let r = crate::core::search::highlight_ranges(name, &self.query);
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

        if let Some(subtitle) = subtitle_opt {
            div()
                .flex_1()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .font(name_font.clone())
                        .text_size(px(*name_size))
                        .text_color(name_color)
                        .child(name_el),
                )
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
