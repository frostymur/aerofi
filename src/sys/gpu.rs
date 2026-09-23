//! Aggressive GPU memory trimming while the launcher is hidden.
//!
//! GPUI renders through Metal and retains, for the process lifetime:
//! per-window glyph/image texture atlases (grown by everything ever
//! rendered, never shrunk), the window's drawable pool, and an app-wide
//! instance-buffer pool. Ordering the window out releases none of it.
//!
//! Apple's documented lever for process GPU memory is
//! `MTLDevice.recommendedMaxWorkingSetSize`: a budget for how much of the
//! process's GPU memory the driver keeps hot. Everything beyond the
//! budget is evicted (private-storage buffers are compressed/swapped out)
//! and re-faults on next use. We shrink the budget while hidden and
//! restore the default (no limit) on show.
//!
//! Note: on Apple silicon the driver class (`AGX…GDevice`) only
//! implements the getter — the property is read-only there, so this is a
//! no-op on M-series Macs (verified via `respondsToSelector:`, which also
//! guards the send). It works on Intel Macs, where the setter exists.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

unsafe extern "C" {
    /// C entry point of the Metal framework (already linked via GPUI).
    fn MTLCreateSystemDefaultDevice() -> *mut c_void;
}

/// GPU memory budget (bytes) kept hot while the launcher is hidden.
/// The atlases and drawables live beyond this and get evicted; 16 MiB is
/// enough to keep a quick re-show snappy without pinning the footprint.
const HIDDEN_BUDGET: u64 = 16 * 1024 * 1024;

static DEVICE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// Release a Metal device reference taken via `MTLCreateSystemDefaultDevice`
/// (Metal uses manual reference retention, so a `+1` is balanced by a
/// `release`).
unsafe fn release_device(dev: *mut c_void) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let obj = dev as *const AnyObject;
    let _: *const AnyObject = msg_send![obj, release];
}

/// The process-wide default Metal device (the one GPUI renders with).
fn device() -> Option<*mut c_void> {
    let mut dev = DEVICE.load(Ordering::Acquire);
    if dev.is_null() {
        dev = unsafe { MTLCreateSystemDefaultDevice() };
        if dev.is_null() {
            return None;
        }
        // Every racer obtains the same singleton; first write wins. If we
        // lost the race, another thread already stored a device — release our
        // redundant `+1` and use the winner's (the stored ref is kept for the
        // process lifetime, as the device itself is).
        if let Err(existing) = DEVICE.compare_exchange(
            core::ptr::null_mut(),
            dev,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            unsafe { release_device(dev) };
            dev = existing;
        }
    }
    (!dev.is_null()).then_some(dev)
}

unsafe fn set_budget(bytes: u64) {
    let Some(dev) = device() else {
        return;
    };
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Sel};
    let obj = dev as *const AnyObject;
    // The Apple-silicon driver class does not implement the setter;
    // check before sending so the call stays a safe no-op there.
    let set_sel = Sel::register("setRecommendedMaxWorkingSetSize:");
    let supported: bool = msg_send![obj, respondsToSelector: set_sel];
    if !supported {
        return;
    }
    // `setRecommendedMaxWorkingSetSize:` — macOS 10.15+. 0 = no limit.
    let _: () = msg_send![obj, setRecommendedMaxWorkingSetSize: bytes];
}

/// Shrink the GPU working-set budget to [`HIDDEN_BUDGET`] (call on hide).
pub fn trim_gpu_memory() {
    unsafe { set_budget(HIDDEN_BUDGET) };
}

/// Restore the default (unlimited) GPU working-set budget (call on show).
pub fn restore_gpu_memory() {
    unsafe { set_budget(0) };
}
