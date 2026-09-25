//! Window setup (borderless PopUp panel) and show/hide focus management.
//!
//! Owns the visibility flag and the GPUI re-render hook; the raw AppKit
//! calls live in [`crate::sys::appkit`].

use gpui::{
    App, AsyncApp, Bounds, Entity, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, point, prelude::*, px, size,
};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::config::AppConfig;
use crate::core::history::History;
use crate::core::item::Target;
use crate::core::theme::ThemeConfig;
use crate::sys::appkit;
use crate::ui::launcher::Launcher;

/// Whether the launcher window is currently visible.
static VISIBLE: AtomicBool = AtomicBool::new(false);

/// Handle used to ask the GPUI loop to redraw the launcher.
///
/// A system-level `NSApp.hide`/`unhide` is not observed by GPUI, so after the
/// window is re-shown its surface can be stale/empty. We ask the loop to re-render
/// explicitly. Both the hotkey handler and the GPUI loop run on the main thread,
/// so a `thread_local` (not a `static`) is enough to share this.
#[derive(Clone)]
struct RenderRequest {
    app: AsyncApp,
    view: Entity<Launcher>,
}

std::thread_local! {
    static RENDER_REQUEST: std::cell::RefCell<Option<RenderRequest>> =
        const { std::cell::RefCell::new(None) };
}

/// Ask the GPUI main loop to re-render the launcher view.
fn request_render() {
    let Some(rr) = RENDER_REQUEST.with(|r| r.borrow().clone()) else {
        return;
    };
    let view = rr.view.clone();
    rr.app.update(|cx| {
        view.update(cx, |_, cx| cx.notify());
    });
}

/// Whether the launcher window is currently visible.
pub fn is_visible() -> bool {
    VISIBLE.load(Ordering::SeqCst)
}

/// Hide the launcher window, returning focus to the previously active app.
/// Must be called on the main thread.
pub fn hide() {
    VISIBLE.store(false, Ordering::SeqCst);
    appkit::hide_application();
}

/// Hide only the launcher window, but keep the application active.
/// Used for compact scripts so the Toast window remains visible.
pub fn hide_launcher_only() {
    VISIBLE.store(false, Ordering::SeqCst);
    // The launcher (and its inline subtitles) is off-screen; stop pacing.
    crate::core::scheduler::notify_visibility(false);
    appkit::hide_launcher_window();
}

/// Toggle the launcher window. Invoked by the global hotkey on the main thread.
pub fn toggle() {
    if is_visible() {
        notify_hide();
        hide();
    } else {
        VISIBLE.store(true, Ordering::SeqCst);
        notify_show();
        appkit::show_application();
        // GPUI doesn't observe the system un-hide, so force a fresh frame.
        request_render();
    }
}

/// Run a `[bindings.global]` target from its hotkey.
///
/// Rofi-mode scripts need the launcher window for their two-way session, so
/// they are opened as a proper Rofi session (window shown + `StartRofiSession`)
/// instead of being spawned detached with nowhere to display. Everything
/// else runs detached via the executor, as before. Invoked by the global
/// hotkey handler on the main thread.
pub fn launch_global_target(target: &Target) {
    let is_rofi = matches!(
        target,
        Target::Script {
            mode: crate::core::item::ScriptMode::Rofi,
            ..
        }
    );
    if !is_rofi {
        // Builtin actions mutate the launcher itself and have no effect in
        // the fire-and-forget executor (it would silently drop them), so
        // route them through the view — otherwise global bindings like
        // `"ctrl+alt+r" = "Reload Configuration"` never fire.
        if matches!(target, Target::Builtin { .. }) {
            if let Some(rr) = RENDER_REQUEST.with(|r| r.borrow().clone()) {
                let view = rr.view.clone();
                rr.app.update(|cx| {
                    view.update(cx, |launcher, cx| {
                        launcher.reload();
                        cx.notify();
                    });
                });
            }
            return;
        }
        crate::core::executor::execute(target);
        return;
    }
    let Some(rr) = RENDER_REQUEST.with(|r| r.borrow().clone()) else {
        crate::core::executor::execute(target);
        return;
    };
    // Start the session first; the launcher shows the window once it has
    // entered Rofi mode and pre-sized it (see `show_for_rofi`), so the first
    // visible frame is already the Rofi layout — not a flash of the search
    // list at the search size that then visibly shrinks ("resize on the go").
    // Same `perform_action` path as picking the script from the search list.
    let view = rr.view.clone();
    let target = target.clone();
    rr.app.update(|cx| {
        view.update(cx, |launcher, cx| {
            launcher.perform_action(
                crate::ui::launcher::LauncherAction::StartRofiSession(target, Vec::new()),
                false,
                cx,
            );
            cx.notify();
        });
    });
}

/// Show the launcher window (AppKit only: un-hide the app, mark visible).
///
/// Intended for a Rofi session launched while hidden (global hotkey): the
/// launcher calls this once it has entered Rofi mode and pre-sized the hidden
/// window, so the first visible frame is already the Rofi layout. The caller
/// already holds the launcher (inside an `App` update), so it runs
/// `on_show` itself and notifies — this function must not re-enter
/// `App::update`.
pub fn show_window() {
    VISIBLE.store(true, Ordering::SeqCst);
    appkit::show_application();
}

/// Drop decoded GPU texture references held by the launcher so macOS can
/// reclaim the memory while hidden.  Safe to call from any context that
/// can reach the GPUI event loop (e.g. `view.update()`).
pub fn notify_hide() {
    // Stop pacing the inline daemons; they stay asleep while hidden.
    crate::core::scheduler::notify_visibility(false);
    let Some(rr) = RENDER_REQUEST.with(|r| r.borrow().clone()) else {
        return;
    };
    let view = rr.view.clone();
    rr.app.update(|cx| {
        view.update(cx, |launcher, cx| {
            launcher.on_hide();
            cx.notify();
        });
    });
}

/// Rebuild the filtered list so the next render creates fresh `img()`
/// elements that GPUI will decode on demand.
pub fn notify_show() {
    // Resume pacing and poke the daemons for fresh inline output.
    crate::core::scheduler::notify_visibility(true);
    let Some(rr) = RENDER_REQUEST.with(|r| r.borrow().clone()) else {
        return;
    };
    let view = rr.view.clone();
    rr.app.update(|cx| {
        view.update(cx, |launcher, cx| {
            launcher.on_show();
            cx.notify();
        });
    });
}

/// Create the borderless PopUp launcher window and return its root view.
///
/// PopUp => non-activating NSPanel at NSPopUpWindowLevel with
/// CanJoinAllSpaces. This is the Raycast/Sol-style window that tiling WMs
/// (aerospace) ignore, so it won't get tiled.
pub fn create_launcher_window(
    cx: &mut App,
    targets: Vec<Target>,
    theme: ThemeConfig,
    app_config: AppConfig,
    history: History,
) -> Entity<Launcher> {
    let screen_w = cx
        .displays()
        .first()
        .map(|d| d.bounds().size.width.as_f32())
        .unwrap_or(1920.0);
    let screen_h = cx
        .displays()
        .first()
        .map(|d| d.bounds().size.height.as_f32())
        .unwrap_or(1080.0);

    let mut bounds = Bounds::centered(
        None,
        size(
            px(theme.window.width.resolve(screen_w)),
            px(theme.window.height.resolve(screen_h)),
        ),
        cx,
    );
    if theme.window.x_offset != 0.0 || theme.window.y_offset != 0.0 {
        bounds.origin += point(px(theme.window.x_offset), px(theme.window.y_offset));
    }
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: WindowKind::PopUp,
                titlebar: None,
                is_movable: false,
                focus: false,
                show: false,
                window_background: if theme.window.blur {
                    WindowBackgroundAppearance::Blurred
                } else {
                    WindowBackgroundAppearance::Transparent
                },
                ..Default::default()
            },
            |window, cx| {
                appkit::store_ns_window(window);
                appkit::set_borderless_style(window, theme.window.corner_radius);

                cx.new(|cx| {
                    cx.observe_window_activation(window, |launcher: &mut Launcher, window, cx| {
                        if !window.is_window_active() {
                            crate::ui::window::hide();
                            launcher.on_hide();
                            cx.notify();
                        }
                    })
                    .detach();
                    Launcher::new(targets, theme, app_config, history)
                })
            },
        )
        .unwrap();

    // The root view entity, used to route keystrokes into the launcher.
    let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();

    // Remember how to force a re-render when the window is re-shown.
    RENDER_REQUEST.with(|r| {
        *r.borrow_mut() = Some(RenderRequest {
            app: cx.to_async(),
            view: view.clone(),
        })
    });

    view
}
