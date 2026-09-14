# Example aerofi Native Plugins (Modi)

This directory contains reference implementations of dynamic C ABI plugins for aerofi:

1. **`web_search`**:
   - **Trigger prefix:** `g ` (e.g. `g rust async`)
   - **Features:** Instantly generates search options for Google and DuckDuckGo and opens them in your default browser.
   - **Non-blocking:** Spawns `open <url>` asynchronously without blocking the launcher UI.

2. **`file_search`**:
   - **Trigger prefix:** `f ` (e.g. `f document.pdf`)
   - **Features:** Uses macOS Spotlight (`mdfind`) to locate files on your system.
    - **Actions:**
      - `Enter`: Open file in default application (`action_code` 0 — what the launcher dispatches).
      - Reveal in Finder (`open -R`, `action_code` 1): implemented by the plugin, but not currently mapped to a key in the launcher.

## Building and Installing

Run from the repository root:

```bash
cargo build --package plugin-web-search --package plugin-file-search --release

mkdir -p ~/.config/aerofi/plugins
cp target/release/libplugin_web_search.dylib ~/.config/aerofi/plugins/
cp target/release/libplugin_file_search.dylib ~/.config/aerofi/plugins/
```

*(Note: If you are inside `examples/plugins/`, the compiled libraries are in `../../target/release/`)*
