use gpui::{
    App, Bounds, Context, Entity, Render, WindowBounds, WindowKind, WindowOptions, div, prelude::*,
    px, rgb,
};

use crate::core::theme::{ThemeConfig, parse_hex_color};

#[derive(Clone, Debug, PartialEq)]
pub enum ToastState {
    Running {
        title: String,
    },
    Done {
        title: String,
        text: String,
        is_error: bool,
    },
}

pub struct ToastWindow {
    theme: ThemeConfig,
    state: ToastState,
}

impl ToastWindow {
    pub fn new(theme: ThemeConfig, title: String) -> Self {
        Self {
            theme,
            state: ToastState::Running { title },
        }
    }

    pub fn set_done(&mut self, text: String, is_error: bool) {
        let final_title = if is_error {
            "Script failed".to_string()
        } else {
            "Script finished running".to_string()
        };

        self.state = ToastState::Done {
            title: final_title,
            text,
            is_error,
        };
    }
}

impl Render for ToastWindow {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;
        let pad = t.element.padding.first().copied().unwrap_or(8.0);
        let window_bg = rgb(parse_hex_color(&t.window.background).unwrap_or(0));
        let text_color = rgb(parse_hex_color(&t.element.text_color).unwrap_or(0));
        let desc_color = rgb(parse_hex_color(
            t.element
                .description_color
                .as_deref()
                .unwrap_or(&t.element.text_color),
        )
        .unwrap_or(0));

        let mut root = div()
            .size_full()
            .flex()
            .items_center()
            .gap_4()
            .px(px(pad + 8.0))
            .py(px(pad + 4.0))
            .bg(window_bg)
            .text_color(text_color)
            .rounded(px(t.window.corner_radius))
            .overflow_hidden();

        if t.window.border_width > 0.0 {
            let border_color = parse_hex_color(&t.window.border_color).unwrap_or(0);
            root = root
                .border(px(t.window.border_width))
                .border_color(rgb(border_color));
        }

        match &self.state {
            ToastState::Running { title } => {
                root = root
                    .child(
                        div()
                            .w(px(10.0))
                            .h(px(10.0))
                            .rounded_full()
                            .bg(rgb(0xaaaaaa)), // neutral dot for running
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(text_color)
                                    .child("Running..."),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(desc_color)
                                    .child(title.to_string()),
                            ),
                    );
            }
            ToastState::Done {
                title,
                text,
                is_error,
            } => {
                let dot_color: u32 = if *is_error { 0xf7768e } else { 0x9ece6a };
                root = root
                    .child(
                        div()
                            .w(px(10.0))
                            .h(px(10.0))
                            .rounded_full()
                            .bg(rgb(dot_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(text_color)
                                    .child(title.to_string()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(desc_color)
                                    .child(text.to_string()),
                            ),
                    );
            }
        }
        root
    }
}

pub fn open_toast_window(
    cx: &mut App,
    theme: ThemeConfig,
    title: String,
) -> (gpui::AnyWindowHandle, Entity<ToastWindow>) {
    let display = cx.displays().into_iter().next();
    let bounds = if let Some(d) = display {
        let b = d.bounds();
        Bounds::new(
            gpui::point(
                b.origin.x + b.size.width / 2.0 - px(250.0), // center horizontally
                b.origin.y + b.size.height - px(150.0),      // 150px from bottom
            ),
            gpui::size(px(500.0), px(72.0)),
        )
    } else {
        Bounds::centered(None, gpui::size(px(500.0), px(72.0)), cx)
    };

    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: WindowKind::PopUp,
                titlebar: None,
                is_movable: false,
                focus: false,
                show: true,
                window_background: gpui::WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |_, cx| cx.new(|_| ToastWindow::new(theme, title)),
        )
        .unwrap();

    let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    (window.into(), view)
}
