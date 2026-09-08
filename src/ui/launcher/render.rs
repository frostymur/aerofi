//! GPUI rendering for the launcher: the `Render` impl and all private
//! view-building helpers (inputbar, listview, grid, full-output, markdown).

use gpui::{
    Context, CursorStyle, Render, Window, div, img, prelude::*, px, rgb, rgba, size, uniform_list,
};

use crate::core::item::Target;
use crate::core::theme::{Widget, parse_hex_color};

use super::helpers::{apply_md_style, expand_tilde_path, format_combo, is_primary_click};
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
    /// Convenience: resolve a theme hex colour string to a `u32` for
    /// GPUI's `rgb()`, falling back to black on bad input.
    fn color(hex: &str) -> u32 {
        parse_hex_color(hex).unwrap_or(0)
    }

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

        Self::with_item_mouse_handlers(
            div()
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
                ),
            format!("cell-{filtered_ix}"),
            filtered_ix,
            cx,
        )
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
    pub(super) fn shortcut_label(&self, name: &str) -> Option<String> {
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
