//! Pure business logic: target parsing, directory indexing, fuzzy search,
//! execution. Knows nothing about GPUI or platform FFI.

pub mod config;
pub mod executor;
pub mod fit;
pub mod fs_watcher;
pub mod history;
pub mod item;
pub mod markdown;
pub mod pango;
pub mod plugin_manager;
pub mod rofi_protocol;
pub mod rofi_session;
pub mod scanner;
pub mod scheduler;
pub mod search;
pub mod theme;
pub mod widget;
pub mod widget_state;
