//! System calls (macOS-only for v0.1): global hotkey, AppKit window
//! control, and native icon extraction. Everything OS-specific lives
//! here; other layers call into it.

pub mod appkit;
pub mod carbon;
pub mod icons;
