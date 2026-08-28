//! System calls (macOS-only for v0.1): global hotkey and AppKit window
//! control. Everything OS-specific lives here; other layers call into it.

pub mod appkit;
pub mod carbon;
