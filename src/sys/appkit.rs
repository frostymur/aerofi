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
///
/// The pointer is kept alive by a process-lifetime `+1` reference taken in
/// [`get_ns_window`]. The launcher window is created once and never
/// destroyed, so holding that reference guarantees the pointer — and every
/// in-flight GCD callback that dereferences it — can never dangle. (If a
/// close/recreate capability is ever added, the retained reference would
/// have to be released before the window goes away.)
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
    let ptr = &*ns_window as *const NSWindow as *mut c_void;
    // Keep the NSWindow alive for the process lifetime so the raw pointer
    // (and every GCD callback that dereferences it) can never dangle. The
    // OS releases the extra reference at process exit.
    std::mem::forget(ns_window);
    Some(ptr)
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
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

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
/// Used during state transitions (e.g. exiting a Rofi script) to resize the
/// window *before* the next render pass.  Without this, the async
/// `window.resize()` in `render()` leaves a one-frame gap where the old
/// content gets scaled by CoreAnimation to the new window size, producing
/// a visible stretch artifact (most noticeable when a theme has a
/// full-height artwork image on the left pane).
///
/// Must be called on the main thread, outside of a draw pass (i.e. from
/// an event handler, not from `render()`).
/// Set the window's opacity (0.0 = invisible, 1.0 = fully visible).
///
/// Used to reveal a freshly shown Rofi window only once its content has been
/// drawn: the window is shown at alpha 0 (so the stale search frame it still
/// holds is not visible, squished to the new size) and brought to alpha 1 on
/// the first Rofi render.
pub fn set_window_alpha(alpha: f64) {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    unsafe {
        use objc2::msg_send;
        let ns_window = &*(ptr as *const NSWindow);
        let _: () = msg_send![ns_window, setAlphaValue: alpha];
    }
}

/// Generation counter for the reveal safety net. Arming a new transition
/// bumps it (invalidating any in-flight timer), as does revealing through
/// the normal path.
static REVEAL_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Arm a safety-net opacity restore: bring the window back to alpha 1.0 on
/// the main queue after `delay_ms`, unless a newer transition has been
/// armed or the normal reveal path has since fired (see
/// [`invalidate_reveal_safety_net`]).
///
/// The normal reveal path (in `render`) waits for a frame whose viewport
/// matches the target size — but an idle window stops rendering frames, so
/// if nothing redraws, the window could otherwise be left invisible
/// forever. The timer guarantees it comes back no matter what.
pub fn arm_reveal_safety_net(delay_ms: u32) {
    extern "C" fn do_reveal(ctx: *mut std::ffi::c_void) {
        let token = ctx as u64;
        if token == REVEAL_GENERATION.load(Ordering::SeqCst) {
            let ptr = NS_WINDOW.load(Ordering::SeqCst);
            if !ptr.is_null() {
                unsafe {
                    use objc2::msg_send;
                    let ns_window = &*(ptr as *const NSWindow);
                    let _: () = msg_send![ns_window, setAlphaValue: 1.0f64];
                }
            }
        }
    }
    unsafe extern "C" {
        // `dispatch_get_main_queue()` is a C inline function with no linker
        // symbol; `_dispatch_main_q` is the underlying dispatch_queue_t object.
        static _dispatch_main_q: std::ffi::c_void;
        // `dispatch_after_f` (not `dispatch_after`, which takes a block):
        // the context-pointer variant of a delayed main-queue callback.
        fn dispatch_after_f(
            when: u64,
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: extern "C" fn(*mut std::ffi::c_void),
        );
        fn dispatch_time(start: u64, delta: i64) -> u64;
    }
    let token = REVEAL_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    unsafe {
        let when = dispatch_time(0, i64::from(delay_ms) * 1_000_000);
        dispatch_after_f(
            when,
            &raw const _dispatch_main_q,
            token as *mut std::ffi::c_void,
            do_reveal,
        );
    }
}

/// Invalidate an armed reveal safety-net timer. Called when the window was
/// revealed through the normal path (so a stray timer can't flash the
/// window) — arming a newer transition also invalidates it implicitly.
pub fn invalidate_reveal_safety_net() {
    REVEAL_GENERATION.fetch_add(1, Ordering::SeqCst);
}

// Pending arguments for [`resize_window_deferred`]. Carried in a
// thread-local (set and consumed on the main thread) because the GCD
// context pointer can only hold one word. If a second resize is queued
// before the first trampoline runs, the latest value wins — transitions
// always converge on the most recent target, so that is the value we need.
thread_local! {
    static PENDING_RESIZE: std::cell::RefCell<Option<(f64, f64, f64, f64)>> =
        const { std::cell::RefCell::new(None) };
}

/// Resize *and* re-center the window to `width`×`height`, deferred to the
/// next main-queue drain (i.e. **outside** the current GPUI `App` update).
///
/// This must not run synchronously from within an `App` update (keystroke
/// handlers, burst handlers): the `setFrameSize:` trampoline that GPUI
/// installs on its content view calls back into the App to run
/// `bounds_changed` via `try_borrow_mut`, which silently fails while the
/// App is already borrowed. A failed callback leaves GPUI's `viewport_size`
/// stale, so the next frame is laid out and painted at the *old* size —
/// with a larger window, the uncovered region keeps showing stale content
/// ("half the window doesn't render"). Dispatching to the main queue runs
/// the resize between App updates, so the callback succeeds and the
/// viewport matches the new size by the next frame.
///
/// The alpha-0/alpha-1 reveal dance in `render` (`pending_reveal`) hides
/// the frame painted before this resize lands.
pub fn resize_window_deferred(width: f64, height: f64, x_offset: f64, y_offset: f64) {
    PENDING_RESIZE.with(|p| *p.borrow_mut() = Some((width, height, x_offset, y_offset)));
    extern "C" fn do_resize(_ctx: *mut std::ffi::c_void) {
        let Some((w, h, ox, oy)) = PENDING_RESIZE.with(|p| p.borrow_mut().take()) else {
            return;
        };
        set_window_frame_centered(w, h, ox, oy);
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
        dispatch_async_f(&raw const _dispatch_main_q, std::ptr::null_mut(), do_resize);
    }
}

/// Atomically set the window's *content* size to `width`×`height` and
/// re-centre it on its current screen (shifted by `x_offset`/`y_offset`).
///
/// `setContentSize:` alone keeps a corner anchored, so a big size change
/// leaves the window off-centre until a later re-centre — a visible two-step
/// "resize, then jump to the middle". Doing the resize and the re-centre back
/// to back makes the compositor only ever present the final size + position.
///
/// Uses `setContentSize:` rather than `setFrame:`: the launcher is a titled
/// panel (`NSTitledWindowMask | NSFullSizeContentViewWindowMask`), so its
/// frame is bigger than its content rect and `setFrame:` would undersize the
/// content, clipping the last list row.
///
/// Must be called on the main thread. Works on a hidden window, so the window
/// can be sized to its final layout *before* it is shown.
pub fn set_window_frame_centered(width: f64, height: f64, x_offset: f64, y_offset: f64) {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_app_kit::NSWindow;
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        let ns_window = &*(ptr as *const NSWindow);
        // 1. Content size (not frame) — see the doc comment.
        let _: () = msg_send![ns_window, setContentSize: NSSize { width, height }];

        // 2. Re-centre using the frame the resize just produced.
        let mut screen: *mut AnyObject = msg_send![ns_window, screen];
        if screen.is_null() {
            screen = msg_send![objc2::class!(NSScreen), mainScreen];
        }
        if screen.is_null() {
            return;
        }
        let screen_frame: NSRect = msg_send![screen, frame];
        let window_frame: NSRect = msg_send![ns_window, frame];
        let new_x = screen_frame.origin.x
            + (screen_frame.size.width - window_frame.size.width) / 2.0
            + x_offset;
        let new_y = screen_frame.origin.y
            + (screen_frame.size.height - window_frame.size.height) / 2.0
            + y_offset;
        let _: () = msg_send![ns_window, setFrameOrigin: NSPoint::new(new_x, new_y)];
    }
}

pub fn screen_width() -> f32 {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return 1920.0;
    }
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_app_kit::NSWindow;
        use objc2_foundation::NSRect;

        let ns_window = &*(ptr as *const NSWindow);
        let mut screen: *mut AnyObject = msg_send![ns_window, screen];
        if screen.is_null() {
            screen = msg_send![objc2::class!(NSScreen), mainScreen];
        }
        if !screen.is_null() {
            let screen_frame: NSRect = msg_send![screen, frame];
            return screen_frame.size.width as f32;
        }
    }
    1920.0
}

pub fn screen_height() -> f32 {
    let ptr = NS_WINDOW.load(Ordering::SeqCst);
    if ptr.is_null() {
        return 1080.0;
    }
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_app_kit::NSWindow;
        use objc2_foundation::NSRect;

        let ns_window = &*(ptr as *const NSWindow);
        let mut screen: *mut AnyObject = msg_send![ns_window, screen];
        if screen.is_null() {
            screen = msg_send![objc2::class!(NSScreen), mainScreen];
        }
        if !screen.is_null() {
            let screen_frame: NSRect = msg_send![screen, frame];
            return screen_frame.size.height as f32;
        }
    }
    1080.0
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

/// Target icon size for downsampling (pixels). 96 covers the list view at
/// Retina (44px) with room to spare; grid themes at 72pt (144px Retina) are
/// upscaled ~1.5x — mildly soft but acceptable. Each cached TIFF is ~36 KB.
const ICON_SIZE: u32 = 96;

/// Fetch the raw multi-resolution TIFF for an `.app` bundle from AppKit.
/// Fast, but must run on the main thread (AppKit / MainThreadMarker). The
/// heavy downsample is split out into [`process_icon_tiff`] so it can run
/// on a worker thread.
pub fn raw_icon_for_app_bundle(path: &Path) -> Option<Vec<u8>> {
    let _mtm = MainThreadMarker::new()?;
    let path_str = NSString::from_str(path.to_str()?);
    let workspace = unsafe { NSWorkspace::sharedWorkspace() };
    let image = unsafe { workspace.iconForFile(&path_str) };

    // Get the raw multi-resolution TIFF from AppKit.
    let tiff_data = unsafe { image.TIFFRepresentation() }?;
    Some(tiff_data.bytes().to_vec())
}

/// Downsample a raw AppKit TIFF to 96×96 via the `image` crate so each
/// cached TIFF is ~36 KB. Pure image processing — thread-safe, no main
/// thread requirement. Returns `None` on decode/encode failure.
pub fn process_icon_tiff(raw_bytes: &[u8]) -> Option<Vec<u8>> {
    // Decode the full-res TIFF.
    let img = image::load_from_memory(raw_bytes).ok()?;

    // Resize to 96×96 using Lanczos3 for quality.
    let resized = img.resize(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);

    // Re-encode as TIFF.
    let mut buf = Cursor::new(Vec::with_capacity(48 * 1024));
    let encoder = TiffEncoder::new(&mut buf);
    let rgba = resized.to_rgba8();
    encoder
        .write_image(&rgba, ICON_SIZE, ICON_SIZE, image::ExtendedColorType::Rgba8)
        .ok()?;

    Some(buf.into_inner())
}
