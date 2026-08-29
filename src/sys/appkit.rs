//! AppKit FFI helpers: chrome stripping and application show/hide.
//!
//! Pure system calls with no knowledge of app state: the visibility flag
//! and the show/hide orchestration live in [`crate::ui::window`].

use gpui::Window;
use objc2::rc::Id;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSView, NSWindow, NSWindowButton,
};
use objc2_foundation::MainThreadMarker;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
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
    // SAFETY: `ns_view` is a valid `NonNull` pointer to the live content `NSView`
    // produced by GPUI's window handle; we only temporarily retain it.
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

/// Strip the macOS traffic-light buttons for a borderless launcher surface.
pub fn hide_chrome(window: &Window) {
    let Some(ptr) = get_ns_window(window) else {
        return;
    };
    let window = unsafe { &*(ptr as *const NSWindow) };
    for button in [
        NSWindowButton::NSWindowCloseButton,
        NSWindowButton::NSWindowMiniaturizeButton,
        NSWindowButton::NSWindowZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(button) {
            button.setHidden(true);
        }
    }
}

/// Make the window non-movable: the transparent titlebar still provides a
/// drag region, so explicitly disable dragging by titlebar and background.
pub fn make_immovable(window: &Window) {
    let Some(ptr) = get_ns_window(window) else {
        return;
    };
    let window = unsafe { &*(ptr as *const NSWindow) };
    window.setMovable(false);
    window.setMovableByWindowBackground(false);
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
