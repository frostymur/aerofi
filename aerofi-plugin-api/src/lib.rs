//! C ABI for Aerofi dynamic plugins.
//!
//! This crate defines the stable `extern "C"` interface that dynamic `.dylib`
//! plugins must implement to extend Aerofi.

use std::ffi::c_char;

/// A single result item returned by a plugin.
#[repr(C)]
pub struct PluginItem {
    /// A unique identifier for this item, used to pass back to `activate`.
    pub id: *const c_char,
    /// The primary text to display in the list.
    pub title: *const c_char,
    /// The secondary text to display in the list.
    pub subtitle: *const c_char,
    /// The icon to display. Follows the same format as GUI session protocol
    /// (e.g., `file:///path/to/icon.png`, `system-icon:Finder`, or emoji).
    /// If null, a default icon is used.
    pub icon: *const c_char,
}

/// A list of results returned by a plugin query.
#[repr(C)]
pub struct PluginResults {
    /// Pointer to the array of items.
    pub items: *const PluginItem,
    /// Number of items in the array.
    pub count: usize,
}

/// Static metadata describing the plugin.
#[repr(C)]
pub struct PluginMetadata {
    /// Internal unique name of the plugin (e.g., `files`).
    pub name: *const c_char,
    /// Human-readable description.
    pub description: *const c_char,
    /// Trigger prefix for the plugin (e.g., `f `).
    pub prefix: *const c_char,
}

/// The main plugin export structure.
/// Every plugin must export a function `aerofi_plugin_init` that returns
/// a pointer to this structure.
#[repr(C)]
pub struct AerofiPlugin {
    /// Must be set to 1.
    pub api_version: u32,

    /// Called once when the plugin is loaded.
    /// Return `true` on success.
    pub init: unsafe extern "C" fn() -> bool,

    /// Called when the plugin is unloaded or the application exits.
    pub destroy: unsafe extern "C" fn(),

    /// Retrieve the plugin's metadata.
    pub get_metadata: unsafe extern "C" fn() -> PluginMetadata,

    /// Query the plugin for results.
    /// The plugin allocates the `PluginResults` and its contents.
    pub query: unsafe extern "C" fn(query: *const c_char) -> PluginResults,

    /// Free the memory of the `PluginResults` previously returned by `query`.
    pub free_results: unsafe extern "C" fn(results: PluginResults),

    /// Called when the user activates an item.
    /// `id` is the `PluginItem::id` that was activated.
    /// `action_code` is 0 for Enter, 1 for Cmd+Enter, etc.
    /// Return `true` to hide the launcher, `false` to keep it open.
    /// **WARNING:** This function must not block! Any heavy work must be
    /// spawned on a separate thread.
    pub activate: unsafe extern "C" fn(id: *const c_char, action_code: u32) -> bool,
}

/// Special action constants that can be returned or used by plugins.
pub mod actions {
    /// Special item ID that triggers an immediate configuration and theme reload in aerofi.
    pub const RELOAD_CONFIG: &str = "aerofi:reload";
}
