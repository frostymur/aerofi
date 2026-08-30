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
pub fn set_borderless_style(window: &Window) {
    // We need both the NSWindow and the NSView (GPUI's native view).
    let handle = HasWindowHandle::window_handle(window).ok();
    let Some(handle) = handle else { return };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return };

    let ns_view_ptr = appkit.ns_view.as_ptr();
    let Some(ns_view) = (unsafe { Id::<NSView>::retain(ns_view_ptr.cast()) }) else {
        return;
    };
    let Some(ns_window) = ns_view.window() else { return };

    // Strip Titled + FullSizeContentView (cause OS-level rounded corners),
    // keep NonactivatingPanel so the panel doesn't steal app focus.
    ns_window.setStyleMask(NSWindowStyleMask::NonactivatingPanel);
    ns_window.setMovable(false);
    ns_window.setMovableByWindowBackground(false);

    // Immediately restore GPUI's native view as first responder.
    // Without this, setStyleMask leaves firstResponder = nil and both
    // AppKit's internal restoration path AND GPUI's own makeFirstResponder_
    // call fire, delivering every key event twice.
    ns_window.makeFirstResponder(Some(&*ns_view));
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

/// Target icon size for downsampling (pixels).
const ICON_SIZE: u32 = 64;

/// Extract the icon for an `.app` bundle, downsampled to 64×64 via the
/// `image` crate so each cached TIFF is ~16 KB. Returns `None` on failure.
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
