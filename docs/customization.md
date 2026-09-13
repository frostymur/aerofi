# aerofi Customization Guide

aerofi is configured via simple, transparent TOML files located in `~/.config/aerofi/`. There are no hidden binary states or opaque databases—everything from keyboard shortcuts to window glassmorphism is version-controllable and human-readable.

---

## 📁 Directory Structure

On first launch, aerofi automatically creates the following layout under `~/.config/aerofi/`:

```
~/.config/aerofi/
├── config.toml         # Primary application configuration
├── scripts/            # User scripts & Raycast-compatible commands
├── themes/             # Custom theme definitions (.toml)
└── plugins/            # Native dynamic C ABI plugins (.dylib)
```

---

## ⚙️ Primary Configuration (`config.toml`)

The main configuration file is located at `~/.config/aerofi/config.toml`.

### Quick Reload
To apply changes to `config.toml` or themes without restarting aerofi:
- Press `Cmd+R` while the launcher is open, or
- Type `Reload Configuration` in the search bar and press `Enter`.

---

### Reference Configuration

Here is an annotated, production-ready `config.toml`:

```toml
# ==============================================================================
# aerofi Configuration Reference
# ==============================================================================

# Active theme: matches a file in ~/.config/aerofi/themes/{theme}.toml
# Built-in options: "default", "tokyo-night", "gruvbox"
theme = "tokyo-night"

[general]
# Global hotkey to toggle aerofi.
# Modifiers: "opt", "cmd", "ctrl", "shift"
# Examples: "opt+space", "cmd+space", "ctrl+shift+p"
toggle_hotkey = "opt+space"

# Maximum number of items rendered simultaneously in the search list
max_results = 20

# Command used to open scripts for editing (e.g. vim, nvim, code, zed)
# Opened in a new Terminal window.
editor = "nvim"

[sources]
# Toggle which sources are indexed and searchable
apps = true
scripts = true
system_settings = true

[scripts]
# Directories scanned recursively for executable scripts.
# The tilde "~" expands to your home directory.
dirs = [
    "~/.config/aerofi/scripts",
    "~/scripts"
]

[apps]
# Patterns to ignore when indexing macOS application bundles.
# Supports glob wildcards ("*" and "?").
ignored = [
    "Uninstall*",
    "Installer",
    "QuickTime Player",
    "*Helper"
]

[aliases]
# Fast keyword triggers.
# Typing an alias finds the target; typing the alias exactly launches
# the target immediately without pressing Enter.
"rc" = "Reload Configuration"
"cb" = "Clipboard History"
"term" = "Ghostty"

[shortcuts]
# In-launcher shortcuts: executed immediately when pressed inside aerofi.
# Modifiers: cmd, ctrl, alt (or opt), shift
"cmd+r" = "Reload Configuration"
"cmd+," = "Open Configuration"

[global_shortcuts]
# System-wide shortcuts registered at startup via Carbon FFI.
# These trigger the target directly from anywhere without opening the search UI.
"opt+c" = "Clipboard History"

[custom_keys]
# Custom keybindings forwarded as custom action codes (10..28) to GUI scripts.
# See docs/scripts.md for details on the interactive GUI protocol.
"kb-custom-1" = "alt+1"
"kb-custom-2" = "alt+2"
```

---

## 🎨 Theming Engine

aerofi features a native styling engine built directly on top of Metal and GPUI. Themes are defined as standalone `.toml` files inside `~/.config/aerofi/themes/`.

### Activating a Theme

To switch themes, set the `theme` field in `config.toml` to the file stem (without `.toml`):

```toml
theme = "gruvbox"
```

aerofi looks for `~/.config/aerofi/themes/gruvbox.toml`.

---

### Theme Anatomy

Every visual aspect of aerofi is customizable:

```toml
name   = "Tokyo Night"
author = "aerofi"

[font]
family = "SF Pro Text"
size = 15.0
fallback = ["SF Pro", "Helvetica Neue", "Arial"]

[window]
width = 680.0
height = 450.0
padding = 16.0
background = "$bg"
blur = true                   # Enables native macOS translucent frosted glass
background_opacity = 0.94     # Translucency level (0.0 - 1.0)
corner_radius = 16.0
border_width = 1.0
border_color = "$border"

[inputbar]
height = 46.0
padding = [10.0, 14.0]
background = "$surface"
text_color = "$text"
placeholder = "Search apps, scripts, commands…"
placeholder_color = "$subtle"
corner_radius = 10.0
icon = "❯"
icon_color = "$accent"

[listview]
columns = 1
spacing = 4.0
empty_text = "No matching items found"
empty_text_color = "$subtle"

[element]
padding = [8.0, 12.0]
corner_radius = 8.0
background = "transparent"
text_color = "$text"
description_color = "$subtle"
show_icons = true
icon_size = 22.0

[element.selected]
background = "$surface2"
text_color = "$text"
description_color = "$accent"

[status_colors]
urgent_background = "$urgent"
urgent_text = "#ffffff"
active_background = "$green"
active_text = "#1a1b26"
accent = "$accent"
muted = "$subtle"

[colors]
# Palette alias system: any "$key" in the theme maps to these definitions.
# Colors can be 6-digit (#RRGGBB) or 8-digit (#RRGGBBAA) hex.
bg = "#1a1b26f0"
surface = "#24283b"
surface2 = "#414868"
border = "#41486880"
text = "#c0caf5"
subtle = "#565f89"
accent = "#7aa2f7"
green = "#9ece6a"
urgent = "#f7768e"
```

---

### Color Variables & Alpha Transparency

- **Hex Formats**: Colors support standard `#RGB`, `#RRGGBB`, and `#RRGGBBAA` (8-digit hex for alpha transparency).
- **Variable Expansion**: Define your palette once in the `[colors]` table. Anywhere in the theme, reference them with a leading `$`, e.g. `background = "$surface"` or `border_color = "$border"`.
- **Frosted Glass Blur**: When `window.blur = true`, macOS native window compositor applies real-time background blur behind the window. Combine this with `background_opacity = 0.90..0.96` for an authentic macOS glassmorphism aesthetic.

---

## ⚡ Performance Optimization Tips

1. **Keep `max_results` around 20–30**: Ensures near-zero memory allocation during fuzzy filtering.
2. **Exclude Large Unneeded Folders**: If configuring custom script directories, point directly to specific folders rather than broad directories like `~` or `~/Downloads`.
3. **Use `.dylib` plugins for massive datasets**: For querying thousands of records (e.g. Spotlight indexing or large databases), use aerofi's native C ABI `.dylib` plugin system (see [docs/plugins.md](plugins.md)) to avoid CLI spawn overhead.
