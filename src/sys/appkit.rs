//! AppKit FFI helpers: chrome stripping and application show/hide.
//!
//! Pure system calls with no knowledge of app state: the visibility flag
//! and the show/hide orchestration live in [`crate::ui::window`].

use gpui::Window;
use image::ImageEncoder;
use image::codecs::tiff::TiffEncoder;
use objc2::rc::Id;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSView, NSWindow, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};

/// Raw pointer to the window's `NSWindow`, captured once after creation.
static NS_WINDOW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Resolve the `NSWindow` backing a GPUI window.
fn get_ns_window(window: &Window) -> Option<*mut c_void> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let ns_view_ptr = appkit.ns_view.as_ptr();
    let ns_view: Id<NSView> = unsafe { Id::retain(ns_view_ptr.cast()) }?;
    let ns_window: Id<NSWindow> = ns_view.window()?;
    Some(&*ns_window as *const NSWindow as *mut c_void)
}

/// Remember the `NSWindow` so we can re-focus it when showing.
pub fn store_ns_window(window: &Window) {
    if let Some(ptr) = get_ns_window(window) {
        NS_WINDOW.store(ptr, Ordering::SeqCst);
    }
}

/// Strip `NSTitledWindowMask` from the NSWindow after GPUI creates it.
///
/// GPUI always includes `NSTitledWindowMask` in the style even when
/// `titlebar: None` is passed — and that flag alone causes macOS to apply
/// its own rounded corners at the compositor level, ignoring the GPUI-side
/// `div().rounded(...)` value.
///
/// `setStyleMask` resets the window's first responder to nil; AppKit and
/// GPUI then both try to restore it, creating two concurrent event paths
/// that cause every keystroke to fire twice.  We prevent this by
/// immediately re-making GPUI's native view the first responder ourselves.
///
/// `corner_radius` is applied to the NSWindow's `contentView` layer so
/// that macOS clips the underlying blur / vibrancy view to the same shape
/// as the GPUI div, eliminating the visible square-corner artifacts.
pub fn set_borderless_style(window: &Window, corner_radius: f32) {
    // We need both the NSWindow and the NSView (GPUI's native view).
    let handle = HasWindowHandle::window_handle(window).ok();
    let Some(handle) = handle else { return };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };

    let ns_view_ptr = appkit.ns_view.as_ptr();
    let Some(ns_view) = (unsafe { Id::<NSView>::retain(ns_view_ptr.cast()) }) else {
        return;
    };
    let Some(ns_window) = ns_view.window() else {
        return;
    };

    // Strip Titled + FullSizeContentView (cause OS-level rounded corners),
    // keep NonactivatingPanel so the panel doesn't steal app focus.
    ns_window.setStyleMask(NSWindowStyleMask::NonactivatingPanel);
    ns_window.setMovable(false);
    ns_window.setMovableByWindowBackground(false);

    // Clip the underlying blur/vibrancy layer to the same rounded rect
    // as the GPUI div, so no square-corner artefacts are visible.
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::msg_send;

        let _: () = msg_send![&*ns_window, setHasShadow: false];

        if corner_radius > 0.0 {
            // contentView.layer.cornerRadius = corner_radius
            let content_view: *mut AnyObject = msg_send![&*ns_window, contentView];
            if !content_view.is_null() {
                // Make sure the view is layer-backed.
                let _: () = msg_send![content_view, setWantsLayer: true];
                let layer: *mut AnyObject = msg_send![content_view, layer];
                if !layer.is_null() {
                    let _: () = msg_send![layer, setCornerRadius: corner_radius as f64];
                    let _: () = msg_send![layer, setMasksToBounds: true];
                }
            }
        }
    }

    // Immediately restore GPUI's native view as first responder.
    // Without this, setStyleMask leaves firstResponder = nil and both
    // AppKit's internal restoration path AND GPUI's own makeFirstResponder_
    // call fire, delivering every key event twice.
    ns_window.makeFirstResponder(Some(&*ns_view));
}

/// Dynamically update the corner radius of the window's layer (e.g. on theme reload).
pub fn set_corner_radius(corner_radius: f32) {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ns_window = &*(ptr as *const NSWindow);
        let content_view: *mut AnyObject = msg_send![ns_window, contentView];
        if !content_view.is_null() {
            let layer: *mut AnyObject = msg_send![content_view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: corner_radius as f64];
            }
        }
    }
}

thread_local! {
    static CENTER_OFFSET: std::cell::RefCell<(f64, f64)> =
        const { std::cell::RefCell::new((0.0, 0.0)) };
}

/// Synchronously set the NSWindow's content size.
///
/// Used during state transitions (e.g. exiting a GUI script) to resize the
/// window *before* the next render pass.  Without this, the async
/// `window.resize()` in `render()` leaves a one-frame gap where the old
/// content gets scaled by CoreAnimation to the new window size, producing
/// a visible stretch artifact (most noticeable when a theme has a
/// full-height artwork image on the left pane).
///
/// Must be called on the main thread, outside of a draw pass (i.e. from
/// an event handler, not from `render()`).
pub fn set_window_size(width: f64, height: f64) {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    unsafe {
        use objc2::msg_send;
        let ns_window = &*(ptr as *const NSWindow);
        let _: () = msg_send![ns_window, setContentSize: objc2_foundation::NSSize {
            width,
            height,
        }];
    }
}

/// Centre the window on the main screen, shifted by (`x_offset`,
/// `y_offset`) points (positive = right/down), deferred via GCD so it
/// fires after GPUI has finished processing the current frame (including
/// any pending `window.resize()` call). Calling `[NSWindow center]`
/// synchronously during render would see the old frame size because GPUI
/// queues the resize for after the render pass. Both the caller and the
/// trampoline run on the main thread, so `CENTER_OFFSET` carries the
/// offsets without a heap allocation.
pub fn center_window(x_offset: f64, y_offset: f64) {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    CENTER_OFFSET.with(|o| *o.borrow_mut() = (x_offset, y_offset));
    // GCD trampoline — dispatch_async on the main queue defers this until
    // after the current run-loop iteration (i.e. after GPUI's resize fires).
    extern "C" fn do_center(ctx: *mut std::ffi::c_void) {
        let ptr = ctx as usize;
        if ptr == 0 {
            return;
        }
        unsafe {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            use objc2_app_kit::NSWindow;
            use objc2_foundation::{NSPoint, NSRect};

            let ns_window = &*(ptr as *const NSWindow);
            let mut screen: *mut AnyObject = msg_send![ns_window, screen];
            if screen.is_null() {
                screen = msg_send![objc2::class!(NSScreen), mainScreen];
            }
            if !screen.is_null() {
                let screen_frame: NSRect = msg_send![screen, frame];
                let window_frame: NSRect = msg_send![ns_window, frame];
                let (ox, oy) = CENTER_OFFSET.with(|o| *o.borrow());
                let new_x = screen_frame.origin.x
                    + (screen_frame.size.width - window_frame.size.width) / 2.0
                    + ox;
                let new_y = screen_frame.origin.y
                    + (screen_frame.size.height - window_frame.size.height) / 2.0
                    + oy;
                let new_origin = NSPoint::new(new_x, new_y);
                let _: () = msg_send![ns_window, setFrameOrigin: new_origin];
            } else {
                let _: () = msg_send![ns_window, center];
            }
        }
    }

    unsafe extern "C" {
        // `dispatch_get_main_queue()` is a C inline function with no linker
        // symbol; `_dispatch_main_q` is the underlying dispatch_queue_t object.
        static _dispatch_main_q: std::ffi::c_void;
        fn dispatch_async_f(
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: extern "C" fn(*mut std::ffi::c_void),
        );
    }

    unsafe {
        dispatch_async_f(&raw const _dispatch_main_q, ptr, do_center);
    }
}

/// Run aerofi as a background accessory: no Dock icon, no Cmd-Tab entry,
/// like Raycast/Alfred. Must be called after GPUI's own
/// `applicationDidFinishLaunching` (which forces the Regular policy), i.e.
/// from the `on_finish_launching` closure, and on the main thread.
pub fn hide_from_dock() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Hide the whole application, returning focus to the previously active app
/// (e.g. the terminal). Must be called on the main thread.
pub fn hide_application() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.hide(None);
}

/// Show and focus the application window. Must be called on the main thread.
#[allow(deprecated)] // `activateIgnoringOtherApps` is the correct "steal focus" call here.
pub fn show_application() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    unsafe {
        let app = NSApplication::sharedApplication(mtm);
        app.unhide(None);
        app.activateIgnoringOtherApps(true);
        let ptr = NS_WINDOW.load(Ordering::SeqCst);
        if !ptr.is_null() {
            let window: &NSWindow = &*(ptr as *const NSWindow);
            window.makeKeyAndOrderFront(None);
        }
    }
}

/// Hide only the main launcher window, leaving the application active so other
/// windows (like the toast window) can remain visible.
pub fn hide_launcher_window() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    unsafe {
        let ptr = NS_WINDOW.load(Ordering::SeqCst);
        if !ptr.is_null() {
            let window: &NSWindow = &*(ptr as *const NSWindow);
            window.orderOut(None);
        }
    }
}

/// Target icon size for downsampling (pixels). 128 covers Retina at 64pt
/// (128px); grid themes at 72pt (144px Retina) are slightly upscaled but
/// still crisp. Each cached TIFF is ~64 KB.
const ICON_SIZE: u32 = 128;

/// Extract the icon for an `.app` bundle, downsampled to 128×128 via the
/// `image` crate so each cached TIFF is ~64 KB. Returns `None` on failure.
pub fn icon_for_app_bundle(path: &Path) -> Option<Vec<u8>> {
    let _mtm = MainThreadMarker::new()?;
    let path_str = NSString::from_str(path.to_str()?);
    let workspace = unsafe { NSWorkspace::sharedWorkspace() };
    let image = unsafe { workspace.iconForFile(&path_str) };

    // Get the raw multi-resolution TIFF from AppKit.
    let tiff_data = unsafe { image.TIFFRepresentation() }?;
    let raw_bytes: Vec<u8> = tiff_data.bytes().to_vec();

    // Decode the full-res TIFF.
    let img = image::load_from_memory(&raw_bytes).ok()?;

    // Resize to 128×128 using Lanczos3 for quality.
    let resized = img.resize(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);

    // Re-encode as TIFF.
    let mut buf = Cursor::new(Vec::with_capacity(64 * 1024));
    let encoder = TiffEncoder::new(&mut buf);
    let rgba = resized.to_rgba8();
    encoder
        .write_image(&rgba, ICON_SIZE, ICON_SIZE, image::ExtendedColorType::Rgba8)
        .ok()?;

    Some(buf.into_inner())
}
