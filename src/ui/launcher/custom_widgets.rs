//! Rendering functions for custom widgets defined in `[[widgets]]`.
//!
//! Each `WidgetDef` variant gets its own render method; `Box` containers
//! recurse via `render_custom_widget`. Unknown ids are silently skipped
//! (a warning was already emitted during registry construction).

use gpui::{Context, CursorStyle, div, img, prelude::*, px, rgb, rgba};

use crate::core::item::Target;
use crate::core::theme::{FontWeightSpec, WidgetDef, parse_hex_color, parse_hex_color_alpha};

use super::helpers::{
    expand_tilde_path, format_combo, is_image_path, is_primary_click, resolve_font_weight,
};
use super::state::Launcher;

impl Launcher {
    /// Render a custom widget by id. Returns `None` if the id is unknown.
    pub(super) fn render_custom_widget(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.render_custom_widget_scoped(id, None, cx)
    }

    /// Render a custom widget by id inside a list item row, passing the row
    /// index and target so buttons can perform context-aware actions.
    pub(super) fn render_custom_widget_for_row(
        &self,
        id: &str,
        row_ix: usize,
        item: &Target,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.render_custom_widget_scoped(id, Some((row_ix, item)), cx)
    }

    fn render_custom_widget_scoped(
        &self,
        id: &str,
        row_context: Option<(usize, &Target)>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let def = self.widget_registry.get(id)?;
        Some(self.render_widget_def(def, row_context, cx))
    }

    /// Dispatch to the type-specific renderer.
    fn render_widget_def(
        &self,
        def: &WidgetDef,
        row_context: Option<(usize, &Target)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match def {
            WidgetDef::Text {
                text,
                color,
                font_size,
                font_weight,
                align,
                ..
            } => self.render_widget_text(
                text.as_deref(),
                color.as_deref(),
                *font_size,
                font_weight.as_ref(),
                align.as_deref(),
            ),
            WidgetDef::Icon {
                icon, size, color, ..
            } => self.render_widget_icon(icon, *size, color.as_deref()),
            WidgetDef::Image {
                path,
                width,
                height,
                radius,
                ..
            } => self.render_widget_image(path, *width, *height, *radius),
            WidgetDef::Spacer { .. } => self.render_widget_spacer(),
            WidgetDef::Divider {
                color,
                thickness,
                margin,
                ..
            } => self.render_widget_divider(color.as_deref(), *thickness, *margin),
            WidgetDef::Box {
                orientation,
                gap,
                padding,
                align,
                background,
                radius,
                width,
                height,
                flex,
                children,
                ..
            } => self.render_widget_box(
                orientation.as_deref(),
                *gap,
                padding.as_deref(),
                align.as_deref(),
                background.as_deref(),
                *radius,
                *width,
                *height,
                *flex,
                children,
                row_context,
                cx,
            ),
            WidgetDef::Button {
                id,
                text,
                icon,
                action,
                hotkey,
                color,
                background,
                hover_background,
                hover_color,
                border_color,
                border_width,
                radius,
                padding,
                font_size,
                font_weight,
                gap,
                close,
            } => self.render_widget_button(
                id,
                text.as_deref(),
                icon.as_deref(),
                action.as_deref(),
                hotkey.as_deref(),
                color.as_deref(),
                background.as_deref(),
                hover_background.as_deref(),
                hover_color.as_deref(),
                border_color.as_deref(),
                *border_width,
                *radius,
                padding.as_deref(),
                *font_size,
                font_weight.as_ref(),
                *gap,
                *close,
                row_context,
                cx,
            ),
        }
    }

    fn render_widget_text(
        &self,
        text: Option<&str>,
        color: Option<&str>,
        font_size: Option<f32>,
        font_weight: Option<&FontWeightSpec>,
        align: Option<&str>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let display = text.unwrap_or("");
        let col = color
            .and_then(parse_hex_color)
            .unwrap_or_else(|| parse_hex_color_alpha(&t.element.text_color).unwrap_or(0x000000FF));
        let size = font_size.unwrap_or(t.font.size);

        let mut el = div().text_color(rgb(col)).text_size(px(size));

        if let Some(w) = font_weight {
            el = el.font_weight(resolve_font_weight(&w.as_str()));
        }

        el = match align {
            Some("center") => el.flex().justify_center().items_center(),
            Some("right") => el.flex().justify_end(),
            _ => el, // left / default
        };

        el.child(display.to_string()).into_any()
    }

    fn render_widget_icon(
        &self,
        icon: &str,
        size: Option<f32>,
        color: Option<&str>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let sz = size.unwrap_or(t.element.icon_size);

        let is_image = is_image_path(icon);

        if is_image {
            let resolved = expand_tilde_path(icon);
            img(std::path::PathBuf::from(resolved))
                .w(px(sz))
                .h(px(sz))
                .rounded_sm()
                .into_any()
        } else {
            let col = color.and_then(parse_hex_color).unwrap_or_else(|| {
                parse_hex_color_alpha(&t.element.text_color).unwrap_or(0x000000FF)
            });

            div()
                .text_color(rgb(col))
                .text_size(px(sz))
                .flex()
                .items_center()
                .justify_center()
                .child(icon.to_string())
                .into_any()
        }
    }

    fn render_widget_image(
        &self,
        path: &str,
        width: Option<f32>,
        height: Option<f32>,
        radius: Option<f32>,
    ) -> gpui::AnyElement {
        let resolved = expand_tilde_path(path);
        let mut el = img(std::path::PathBuf::from(resolved));

        if let Some(w) = width {
            el = el.w(px(w));
        }
        if let Some(h) = height {
            el = el.h(px(h));
        }
        if let Some(r) = radius {
            el = el.rounded(px(r));
        }

        el.object_fit(gpui::ObjectFit::Cover).into_any()
    }

    fn render_widget_spacer(&self) -> gpui::AnyElement {
        div().flex_1().into_any()
    }

    fn render_widget_divider(
        &self,
        color: Option<&str>,
        thickness: Option<f32>,
        margin: Option<f32>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let col = color
            .and_then(parse_hex_color)
            .unwrap_or_else(|| parse_hex_color_alpha(&t.window.border_color).unwrap_or(0x888888FF));
        let th = thickness.unwrap_or(1.0);
        let m = margin.unwrap_or(4.0);

        div().w_full().h(px(th)).my(px(m)).bg(rgb(col)).into_any()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_widget_box(
        &self,
        orientation: Option<&str>,
        gap: Option<f32>,
        padding: Option<&[f32]>,
        align: Option<&str>,
        background: Option<&str>,
        radius: Option<f32>,
        width: Option<f32>,
        height: Option<f32>,
        flex: Option<bool>,
        children: &[String],
        row_context: Option<(usize, &Target)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let is_horizontal = orientation.unwrap_or("horizontal") == "horizontal";
        let mut container = div().flex();

        if is_horizontal {
            container = container.flex_row();
        } else {
            container = container.flex_col();
        }

        if let Some(w) = width {
            container = container.w(px(w));
        }
        if let Some(h) = height {
            container = container.h(px(h));
        }
        if flex.unwrap_or(false) {
            container = container.flex_1();
        }

        if let Some(g) = gap {
            container = container.gap(px(g));
        }

        if let Some(pad) = padding {
            let h = pad.first().copied().unwrap_or(0.0);
            let v = pad.get(1).copied().unwrap_or(h);
            container = container.px(px(h)).py(px(v));
        }

        container = match align {
            Some("center") => container.items_center(),
            Some("end") => container.items_end(),
            _ => container.items_start(),
        };

        // Box backgrounds support opaque (#RRGGBB), translucent (#RRGGBBAA),
        // and "transparent", so panels can be semi-transparent.
        if let Some(bg) = background.and_then(parse_hex_color_alpha) {
            container = container.bg(rgba(bg));
        }

        if let Some(r) = radius {
            container = container.rounded(px(r));
        }

        // Recurse into children: supports built-ins (InputBar, ListView) as well as custom widgets.
        for child_id in children {
            if child_id.eq_ignore_ascii_case("inputbar") {
                container = container.child(self.render_inputbar(cx));
            } else if child_id.eq_ignore_ascii_case("listview") {
                match &self.state {
                    crate::ui::launcher::types::LauncherState::Search => {
                        let columns = self.theme.listview.columns.max(1);
                        container = container.child(self.render_listview(cx, columns));
                    }
                    crate::ui::launcher::types::LauncherState::Confirming { target, .. } => {
                        container = container.child(self.render_confirmation(target, cx));
                    }
                    _ => {}
                }
            } else if let Some(child_el) =
                self.render_custom_widget_scoped(child_id, row_context, cx)
            {
                container = container.child(child_el);
            }
        }

        container.into_any()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_widget_button(
        &self,
        id: &str,
        text: Option<&str>,
        icon: Option<&str>,
        action: Option<&str>,
        hotkey: Option<&str>,
        color: Option<&str>,
        background: Option<&str>,
        hover_background: Option<&str>,
        hover_color: Option<&str>,
        border_color: Option<&str>,
        border_width: Option<f32>,
        radius: Option<f32>,
        padding: Option<&[f32]>,
        font_size: Option<f32>,
        font_weight: Option<&FontWeightSpec>,
        gap: Option<f32>,
        close: Option<bool>,
        row_context: Option<(usize, &Target)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = &self.theme;
        let btn_id = match row_context {
            Some((ix, _)) => format!("row-{ix}-widget-button-{id}"),
            None => format!("widget-button-{id}"),
        };
        let mut btn = div()
            .id(btn_id)
            .cursor(CursorStyle::PointingHand)
            .flex()
            .items_center()
            .justify_center();

        if let Some(pad) = padding {
            let h = pad.first().copied().unwrap_or(8.0);
            let v = pad.get(1).copied().unwrap_or(h);
            btn = btn.px(px(h)).py(px(v));
        } else {
            btn = btn.px(px(8.0)).py(px(4.0));
        }

        if let Some(r) = radius {
            btn = btn.rounded(px(r));
        }

        if let Some(bg) = background.and_then(parse_hex_color) {
            btn = btn.bg(rgb(bg));
        }

        let col = color
            .and_then(parse_hex_color)
            .unwrap_or_else(|| parse_hex_color_alpha(&t.element.text_color).unwrap_or(0x000000FF));
        btn = btn.text_color(rgb(col));

        let hbg_opt = hover_background.and_then(parse_hex_color);
        let hc_opt = hover_color.and_then(parse_hex_color);
        if hbg_opt.is_some() || hc_opt.is_some() {
            btn = btn.hover(move |mut s| {
                if let Some(hbg) = hbg_opt {
                    s = s.bg(rgb(hbg));
                }
                if let Some(hc) = hc_opt {
                    s = s.text_color(rgb(hc));
                }
                s
            });
        }

        if let Some(bc) = border_color.and_then(parse_hex_color) {
            let bw = border_width.unwrap_or(1.0);
            btn = btn.border(px(bw)).border_color(rgb(bc));
        }

        let sz = font_size.unwrap_or(t.font.size);
        btn = btn.text_size(px(sz));

        if let Some(w) = font_weight {
            btn = btn.font_weight(resolve_font_weight(&w.as_str()));
        }

        if let Some(g) = gap {
            btn = btn.gap(px(g));
        } else if text.is_some() && icon.is_some() {
            btn = btn.gap(px(6.0));
        }

        if let Some(ic) = icon {
            let is_image = is_image_path(ic);

            if is_image {
                let resolved = expand_tilde_path(ic);
                let ic_sz = font_size.unwrap_or(14.0);
                btn = btn.child(
                    img(std::path::PathBuf::from(resolved))
                        .w(px(ic_sz))
                        .h(px(ic_sz))
                        .rounded_sm(),
                );
            } else {
                btn = btn.child(ic.to_string());
            }
        }

        if let Some(txt) = text {
            btn = btn.child(txt.to_string());
        }

        if let Some(hk) = hotkey {
            let label = format_combo(hk);
            let hk_col = col;
            btn = btn.child(
                div()
                    .ml(px(4.0))
                    .text_size(px(sz - 2.0))
                    .text_color(rgb(hk_col))
                    .opacity(0.6)
                    .child(label),
            );
        }

        if let Some(act) = action {
            let action_string = act.to_string();
            let row_item_clone = row_context.map(|(_, item)| item.clone());
            let stay_open = matches!(close, Some(false));
            btn = btn.on_click(cx.listener(move |this, event, _window, cx| {
                if is_primary_click(event) {
                    this.handle_widget_button_action(
                        &action_string,
                        row_item_clone.as_ref(),
                        stay_open,
                        cx,
                    );
                }
            }));
        }

        btn.into_any()
    }
}
