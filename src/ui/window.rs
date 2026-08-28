//! Window setup (borderless PopUp panel) and show/hide focus management.
//!
//! Owns the visibility flag and the GPUI re-render hook; the raw AppKit
//! calls live in [`crate::sys::appkit`].

use gpui::{
    App, AsyncApp, Bounds, Entity, TitlebarOptions, WindowBounds, WindowKind, WindowOptions,
    prelude::*, px, size,
};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::common::config::Config;
use crate::core::item::Target;
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

/// Toggle the launcher window. Invoked by the global hotkey on the main thread.
pub fn toggle() {
    if is_visible() {
        hide();
    } else {
        VISIBLE.store(true, Ordering::SeqCst);
        appkit::show_application();
        // GPUI doesn't observe the system un-hide, so force a fresh frame.
        request_render();
    }
}

/// Create the borderless PopUp launcher window and return its root view.
///
/// PopUp => non-activating NSPanel at NSPopUpWindowLevel with
/// CanJoinAllSpaces. This is the Raycast/Sol-style window that tiling WMs
/// (aerospace) ignore, so it won't get tiled.
pub fn create_launcher_window(
    cx: &mut App,
    targets: Vec<Target>,
    config: Config,
) -> Entity<Launcher> {
    let bounds = Bounds::centered(
        None,
        size(px(config.window.width), px(config.window.height)),
        cx,
    );
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: WindowKind::PopUp,
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            |window, cx| {
                appkit::hide_chrome(window);
                appkit::store_ns_window(window);
                cx.new(|_| Launcher::new(targets, config.theme))
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
