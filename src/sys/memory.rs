//! Best-effort heap pressure relief.
//!
//! Rust's default allocator is system malloc, which keeps freed pages for
//! reuse: a busy session (theme switches, GUI scripts, re-indexing) leaves
//! an RSS high-water mark in place while the launcher sits idle.
//! `malloc_zone_pressure_relief` (macOS 10.7+, symbol resolved at runtime)
//! asks malloc to munmap some of its free memory back to the kernel.
//!
//! Note: modern macOS (15.x) frequently releases nothing — its internal
//! zones do not expose freed spans to the call, and the function reports
//! 0 bytes. The call is kept because it is cheap and older releases do
//! honor it; it is an advice, never a guarantee.

/// Ask the system allocator to return freed heap pages to the OS.
///
/// Returns the number of bytes the allocator reports as released, or `0`
/// when the allocator declines. The symbol is declared since macOS 10.7
/// but is still resolved with `dlsym` as a runtime safety net. Passes a
/// `NULL` zone so every registered zone is examined, and a `0` goal so
/// the allocator may release as much as it considers safe.
#[cfg(target_os = "macos")]
pub fn pressure_relief() -> usize {
    use std::ffi::c_void;
    type ZonePtr = *mut c_void;
    type ReliefFn = unsafe extern "C" fn(zone: ZonePtr, size: usize) -> usize;

    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    }

    // RTLD_DEFAULT is the -2 sentinel handle on macOS.
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

    unsafe {
        let ptr = dlsym(RTLD_DEFAULT, c"malloc_zone_pressure_relief".as_ptr());
        if ptr.is_null() {
            return 0;
        }
        let relief: ReliefFn = std::mem::transmute(ptr);
        relief(std::ptr::null_mut(), 0)
    }
}

/// Non-macOS stub (the app is macOS-only; keeps other targets compiling).
#[cfg(not(target_os = "macos"))]
pub fn pressure_relief() -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_relief_smoke() {
        // Must never panic or abort, on any macOS version; the return
        // value is an advice outcome, not a guarantee.
        let _bytes = pressure_relief();
    }
}
