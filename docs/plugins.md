# aerofi Native Plugins Guide (`.dylib` C ABI)

aerofi supports dynamic plugins via a native **C ABI shared library (`.dylib`)** interface. This enables you to extend aerofi with deep system integrations, fast file search engines, window management actions, or custom developer workflows without recompiling the core launcher.

---

## Architecture & Performance Philosophy

Most modern launchers rely on heavy JavaScript / TypeScript engines (Node.js, Electron, or quickjs) or RPC socket bridges. While accessible, they incur heavy RSS memory footprints (100MB–400MB) and perceptible typing latencies.

aerofi takes a different approach:
- **Zero Runtime Overhead**: Plugins compile directly to native machine code (`.dylib`) using Rust, C, C++, Zig, or Swift.
- **Microsecond In-Memory Execution**: Queries execute directly in-process via fast C ABI function calls.
- **Strict Non-Blocking Contract**: All plugin work runs asynchronously or off the main thread, guaranteeing that aerofi's 120 FPS UI loop never stutters.

---

## Plugin Discovery

aerofi automatically scans and loads all `.dylib` files on startup from:

```
~/.config/aerofi/plugins/
~/Library/Application Support/aerofi/plugins/   # macOS (also scanned)
```

Whenever aerofi starts, it dynamically loads each shared library, checks the API version handshake, and registers the plugin's trigger prefix.

---

## The Plugin API (`aerofi-plugin-api`)

The `aerofi-plugin-api` crate defines the stable C ABI contracts.

### Core Data Structures

```rust
use std::ffi::c_char;

/// A single result item returned by a plugin.
#[repr(C)]
pub struct PluginItem {
    pub id: *const c_char,        // Unique item identifier
    pub title: *const c_char,     // Primary list row text
    pub subtitle: *const c_char,  // Secondary subtitle text
    pub icon: *const c_char,      // Icon (emoji, file://, system-icon:)
}

/// Query results container.
#[repr(C)]
pub struct PluginResults {
    pub items: *const PluginItem, // Pointer to array of items
    pub count: usize,             // Number of items
}

/// Static plugin metadata.
#[repr(C)]
pub struct PluginMetadata {
    pub name: *const c_char,        // Internal unique name (e.g. "web-search")
    pub description: *const c_char, // Human-readable description
    pub prefix: *const c_char,      // Activation prefix (e.g. "g ")
}
```

---

### The `AerofiPlugin` Interface

Every plugin must export a C-compatible function named `aerofi_plugin_init` that returns a pointer to an `AerofiPlugin` struct:

```rust
#[repr(C)]
pub struct AerofiPlugin {
    pub api_version: u32,                                          // Must be 1
    pub init: unsafe extern "C" fn() -> bool,                      // Called on load
    pub destroy: unsafe extern "C" fn(),                           // Called on unload
    pub get_metadata: unsafe extern "C" fn() -> PluginMetadata,    // Returns metadata
    pub query: unsafe extern "C" fn(query: *const c_char) -> PluginResults,
    pub free_results: unsafe extern "C" fn(results: PluginResults),
    pub activate: unsafe extern "C" fn(id: *const c_char, action_code: u32) -> bool,
}
```

#### Lifecycle Callbacks
- **`init()`**: Called when the `.dylib` is loaded. Returns `true` if initialization succeeded.
- **`destroy()`**: Called when aerofi exits or unloads the plugin. Cleans up any persistent resources.
- **`get_metadata()`**: Returns the plugin's name, description, and trigger prefix.
- **`query(q)`**: Called whenever the user types after the trigger prefix. Allocates and returns `PluginResults`.
- **`free_results(res)`**: Called by aerofi to deallocate the results previously returned by `query`.
- **`activate(id, code)`**: Called when the user hits `Enter` (code 0) or secondary shortcuts. Returns `true` to dismiss the launcher or `false` to keep it open.

---

## Creating a Plugin in Rust

### 1. Configure `Cargo.toml`

Create a new Rust library crate and configure it as a `cdylib`:

```toml
[package]
name = "my-aerofi-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
aerofi-plugin-api = "0.1"
```

The API crate is published on [crates.io](https://crates.io/crates/aerofi-plugin-api)
(the in-repo example plugins use a `path` dependency instead).

---

### 2. Implement the Plugin (`src/lib.rs`)

```rust
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use aerofi_plugin_api::{AerofiPlugin, PluginItem, PluginMetadata, PluginResults};

static PLUGIN: AerofiPlugin = AerofiPlugin {
    api_version: 1,
    init,
    get_metadata,
    query,
    activate,
    free_results,
    destroy,
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aerofi_plugin_init() -> *const AerofiPlugin {
    &PLUGIN
}

unsafe extern "C" fn init() -> bool {
    true
}

unsafe extern "C" fn destroy() {}

unsafe extern "C" fn get_metadata() -> PluginMetadata {
    PluginMetadata {
        name: CString::new("calc").unwrap().into_raw(),
        description: CString::new("Quick math calculation").unwrap().into_raw(),
        prefix: CString::new("= ").unwrap().into_raw(),
    }
}

unsafe extern "C" fn query(query_ptr: *const c_char) -> PluginResults {
    if query_ptr.is_null() {
        return PluginResults { items: ptr::null(), count: 0 };
    }

    let q = unsafe { CStr::from_ptr(query_ptr) }.to_string_lossy();
    // Perform calculation or lookup...
    let item = PluginItem {
        id: CString::new("calc:result").unwrap().into_raw(),
        title: CString::new(format!("Result for: {}", q)).unwrap().into_raw(),
        subtitle: CString::new("Press Enter to copy").unwrap().into_raw(),
        icon: CString::new("emoji:🔢").unwrap().into_raw(),
    };

    let items_boxed = vec![item].into_boxed_slice();
    let count = items_boxed.len();
    let items = Box::into_raw(items_boxed) as *const PluginItem;

    PluginResults { items, count }
}

unsafe extern "C" fn free_results(results: PluginResults) {
    if !results.items.is_null() && results.count > 0 {
        let slice = unsafe {
            Box::from_raw(std::slice::from_raw_parts_mut(
                results.items as *mut PluginItem,
                results.count,
            ))
        };
        for item in slice {
            if !item.id.is_null() { unsafe { drop(CString::from_raw(item.id as *mut _)) }; }
            if !item.title.is_null() { unsafe { drop(CString::from_raw(item.title as *mut _)) }; }
            if !item.subtitle.is_null() { unsafe { drop(CString::from_raw(item.subtitle as *mut _)) }; }
            if !item.icon.is_null() { unsafe { drop(CString::from_raw(item.icon as *mut _)) }; }
        }
    }
}

unsafe extern "C" fn activate(id_ptr: *const c_char, _action_code: u32) -> bool {
    if id_ptr.is_null() {
        return true;
    }
    // Perform action...
    true // Return true to close launcher
}
```

---

### 3. Build & Install

```bash
cargo build --release
mkdir -p ~/.config/aerofi/plugins
cp target/release/libmy_aerofi_plugin.dylib ~/.config/aerofi/plugins/
```

Restart aerofi or press `Cmd+R` to reload configuration.

> **Note:** if you change the example plugins, rebuild their prebuilt universal `.dylib`s and re-sign them so the repo binaries stay in sync (per plugin):
>
> ```bash
> rustup target add x86_64-apple-darwin
> cargo build --release -p plugin-web-search -p plugin-file-search
> cargo build --release --target x86_64-apple-darwin -p plugin-web-search -p plugin-file-search
> lipo -create target/release/libplugin_web_search.dylib \
>   target/x86_64-apple-darwin/release/libplugin_web_search.dylib \
>   -output examples/plugins/web_search/libplugin_web_search.dylib
> codesign -s - -f examples/plugins/web_search/libplugin_web_search.dylib
> ```

---

## Reference Examples in Repository

Explore the ready-to-build examples included in the aerofi repository. Each example also ships a **prebuilt universal (arm64 + x86_64) `.dylib`** right next to its sources — no compilation needed:

```bash
mkdir -p ~/.config/aerofi/plugins
cp examples/plugins/web_search/libplugin_web_search.dylib ~/.config/aerofi/plugins/
cp examples/plugins/file_search/libplugin_file_search.dylib ~/.config/aerofi/plugins/
```

1. **Web Search Plugin** (`examples/plugins/web_search/`):
   - Prefix: `g ` (e.g. `g rust async`)
   - Dynamically generates search shortcuts for Google and DuckDuckGo and opens them asynchronously in your default browser.
2. **Spotlight File Search Plugin** (`examples/plugins/file_search/`):
   - Prefix: `f ` (e.g. `f invoice.pdf`)
    - Queries macOS Spotlight via `mdfind` and opens the selected file. (The plugin also implements a reveal-in-Finder action for `action_code == 1`, though the launcher currently only dispatches the open action.)
