# aerofi-plugin-api

C ABI for [aerofi](https://github.com/frostymur/aerofi) launcher plugins.

aerofi extends through native shared libraries (`.dylib` on macOS) loaded at
startup and queried **in-process on every keystroke** — no process spawning,
no JS runtime. This crate defines the stable `extern "C"` contract plugins
implement: metadata, `query`, `free_results`, `activate`, and the
`api_version` handshake.

Works from Rust, C, C++, Swift, or any language that can emit a C ABI shared
library.

## Usage

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

Minimal plugin (`src/lib.rs`):

```rust
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use aerofi_plugin_api::{AerofiPlugin, PluginItem, PluginMetadata, PluginResults};

static PLUGIN: AerofiPlugin = AerofiPlugin {
    api_version: 1,
    init,
    destroy,
    get_metadata,
    query,
    free_results,
    activate,
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aerofi_plugin_init() -> *const AerofiPlugin {
    &PLUGIN
}

unsafe extern "C" fn init() -> bool { true }
unsafe extern "C" fn destroy() {}

unsafe extern "C" fn get_metadata() -> PluginMetadata {
    PluginMetadata {
        name: CString::new("calc").unwrap().into_raw(),
        description: CString::new("Quick math").unwrap().into_raw(),
        prefix: CString::new("= ").unwrap().into_raw(),
    }
}

unsafe extern "C" fn query(q: *const c_char) -> PluginResults {
    let _text = if q.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(q) }.to_string_lossy().into_owned()
    };
    PluginResults { items: ptr::null(), count: 0 } // real items here
}

unsafe extern "C" fn free_results(_r: PluginResults) {}

unsafe extern "C" fn activate(_id: *const c_char, _action: u32) -> bool {
    true
}
```

Build and install:

```bash
cargo build --release
cp target/release/libmy_aerofi_plugin.dylib ~/.config/aerofi/plugins/
```

Restart aerofi or press `Cmd+R` — type `= ` to trigger the plugin.

## Contract highlights

- `activate` **must not block** — spawn heavy work on a separate thread.
- Memory returned by `query` is owned by the plugin and freed by aerofi via
  `free_results`.
- `api_version` must match what aerofi supports (currently `1`).

## Versioning

The C ABI is the public contract. While the crate is `0.x`, minor versions
may still break the ABI; the `api_version` field guards incompatible
plugins at load time.

See the full guide with working examples:
[docs/plugins.md](https://github.com/frostymur/aerofi/blob/main/docs/plugins.md)
