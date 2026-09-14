# aerofi Customization Guide

aerofi is configured via transparent, human-readable TOML files located in `~/.config/aerofi/`. Everything from global hotkeys and script sources to window glassmorphism, container orientations, and custom UI widgets is fully configurable.

---

## 📑 Table of Contents

- [📁 Directory Structure](#-directory-structure)
- [📄 Complete Reference Files](#-complete-reference-files)
- [⚙️ Application Configuration (`config.toml`)](#️-application-configuration-configtoml)
  - [General Options](#general-options)
  - [Search Sources](#search-sources)
  - [Script Directories](#script-directories)
  - [App Filtering & Custom Discovery](#app-filtering--custom-discovery)
  - [Aliases & Shortcuts](#aliases--shortcuts)
  - [Custom Keys (GUI Mode)](#custom-keys-gui-mode)
- [🎨 Theming Engine (`theme.toml`)](#-theming-engine-themetoml)
  - [Modular Themes & Imports (`imports`)](#modular-themes--imports-imports--)
  - [Font & Typography](#font--typography)
  - [Window & Frosted Glassmorphism](#window--frosted-glassmorphism)
  - [Layout Hierarchy (`[mainbox]`)](#layout-hierarchy-mainbox)
  - [Search Bar (`[inputbar]`)](#search-bar-inputbar)
  - [Results List & Badges (`[listview]`)](#results-list--badges-listview)
  - [Row Slots & Hover States (`[element]`)](#row-slots--hover-states-element)
  - [Semantic Status Colors](#semantic-status-colors)
  - [Floating Toast](#floating-toast)
  - [Palette Variable Aliases (`[colors]`)](#palette-variable-aliases-colors)
- [🧩 Custom Widgets System](#-custom-widgets-system)
  - [Widget Types & Properties](#widget-types--properties)
  - [Composing a Custom Header and Footer](#composing-a-custom-header-and-footer)
- [⚡ Performance Optimization](#-performance-optimization)

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

## 📄 Complete Reference Files

For copy-pasteable reference files documenting **every single parameter and type**, see:
- ⚙️ **[examples/config.toml](../examples/config.toml)** — Complete configuration reference.
- 🎨 **[examples/theme.toml](../examples/theme.toml)** — Complete theme reference featuring custom layouts and widgets.

---

## ⚙️ Application Configuration (`config.toml`)

Path: `~/.config/aerofi/config.toml`

### Quick Reload
To apply changes without restarting aerofi:
- Press `Cmd+R` while aerofi is open, or
- Search for `Reload Configuration` and press `Enter`.

### General Options
```toml
# Active theme. "default" is built in; any other name is loaded from
# ~/.config/aerofi/themes/{name}.toml (must exist).
theme = "tokyo-night"

[general]
# Maximum number of search results displayed simultaneously.
max_results = 20
```

### Search Sources
```toml
[sources]
apps = true            # Applications from /Applications, /System/Applications, ~/Applications
scripts = true         # Scripts from configured script directories
system_settings = true # macOS System Settings panes (reserved)
```

### Script Directories
```toml
[scripts]
# Folders scanned recursively for launchable scripts.
# Leading "~" expands to your home directory.
dirs = [
    "~/.config/aerofi/scripts",
    "~/scripts"
]
```

### App Discovery & Filtering
aerofi indexes standard application directories (`/Applications`, `/System/Applications`, and their `Utilities/` subdirectories such as Activity Monitor, Console, and Terminal) automatically.

You can exclude specific apps, add additional directories (e.g. Homebrew casks), or include individual `.app` bundles directly:

```toml
[apps]
# Filter out unwanted items
ignore_names = ["Uninstall*", "Installer"]
ignore_dirs = ["~/Applications/Chrome Apps.localized"]

# Add specific paths
extra_dirs = ["/opt/homebrew/Applications"]
extra_apps = ["/System/Library/CoreServices/Finder.app"]
```

### Aliases & Shortcuts
```toml
[aliases]
# Immediate launch triggers: typing the exact alias launches the target
# immediately without needing to press Enter.
"rc" = "Reload Configuration"
"cb" = "Clipboard History"
"term" = "Ghostty"
```

### Hotkeys and Bindings
The `[bindings]` table centrally manages all keyboard shortcuts for the launcher:

```toml
[bindings]
# Global hotkey to toggle the launcher visibility
toggle = "opt+space"

[bindings.launcher]
# Shortcuts active only while aerofi is open.
# Triggers the target matching the mapped string.
"cmd+r" = "Reload Configuration"
"cmd+," = "Open Configuration"

[bindings.global]
# System-wide hotkeys registered at startup via Carbon.
# Directly launches the target without opening the search UI.
"opt+c" = "Clipboard History"
```

### Custom Keys (GUI Mode)
```toml
[bindings.custom]
# Forward custom key combinations as action return codes (retv: 10..28)
# to interactive GUI scripts (see docs/scripts.md).
"kb-custom-1" = "alt+1"
"kb-custom-2" = "alt+2"
"kb-custom-3" = "ctrl+d"
```

---

## 🎨 Theming Engine (`theme.toml`)

aerofi themes are defined in standard TOML under `~/.config/aerofi/themes/{theme}.toml`.

### Modular Themes & Imports (`imports = [...]`)

aerofi allows you to cleanly separate colors, window geometry, and widget hierarchies across multiple files using the top-level `imports` array:

```toml
name = "My Custom Theme"

# Import reusable color palette and layout mixins
imports = [
    "colors/tokyo-night.toml",
    "layouts/compact.toml",
]

# Override only specific values for this theme:
[window]
width = 660.0
```

#### Key Rules:
1. **Path Resolution**: Paths in `imports` are resolved relative to `~/.config/aerofi/themes/` (with automatic fallback to `$XDG_CONFIG_HOME/aerofi/themes/` or macOS Application Support).
2. **Recursive Deep Merge**: Tables (such as `[window]`, `[font]`, `[inputbar]`, `[colors]`, and custom `[widgets]`) are merged recursively.
3. **Exclusive Override for Arrays**: Non-table items (such as `children = [...]` in `[mainbox]`, `layout = [...]` in `[element]`, and `fallback = [...]` in `[font]`) are completely replaced by the importing file instead of being concatenated. What you declare in your file is exactly what gets rendered.
4. **Order of Precedence**: Imports are evaluated in order from first to last; the importing file itself has the highest priority and overrides imported values.
5. **Cycle Detection**: Circular references (e.g. A imports B and B imports A) are automatically detected and safely skipped with a warning.

### Font & Typography
```toml
[font]
family = "SF Pro Text"                                    # Font family name
size = 15.0                                              # Base size in points
fallback = ["SF Pro", "SF Mono", "Helvetica Neue", "Arial"] # Fallback glyph fonts
```

### Window & Frosted Glassmorphism
```toml
[window]
width = 720.0
height = 480.0
padding = 16.0
background = "$bg"
blur = true               # Native macOS translucent frosted glass blur
background_opacity = 0.94 # Translucency (0.0 = clear, 1.0 = opaque)
corner_radius = 16.0
border_width = 1.0
border_color = "$border"

# Optional background image:
# background_image = "~/.config/aerofi/themes/wallpaper.jpg"
# background_position = "cover" # "cover" (default), "left", "right"
# image_scale = 1.0
```

### Layout Hierarchy (`[mainbox]`)

aerofi allows you to completely rearrange the main UI layout!

```toml
[mainbox]
# "vertical" (stacked top-to-bottom) or "horizontal" (side-by-side)
orientation = "vertical"

# List of widgets rendered inside the window.
# Can include built-in widgets ("InputBar", "ListView", "Banner")
# OR any custom widget ID defined under [widgets.<id>].
children = [
    "header_bar",
    "InputBar",
    "ListView",
    "footer_bar"
]
```

### Search Bar (`[inputbar]`)
```toml
[inputbar]
height = 46.0
padding = [10.0, 14.0]               # [left/right, top/bottom]
margin = [0.0, 0.0, 8.0, 0.0]        # [top, right, bottom, left]
background = "$surface"
text_color = "$text"
placeholder = "Search apps, scripts, commands…"
placeholder_color = "$subtle"
corner_radius = 10.0
icon = "❯"
icon_color = "$accent"
```

### Results List & Badges (`[listview]`)
```toml
[listview]
columns = 1                          # 1 for list; any value >1 renders a grid (2, 3, 4, …)
spacing = 4.0                        # Gap between rows
scrollbar = false                    # Visible scrollbar
empty_text = "No matching items"
empty_text_color = "$subtle"
# require_input = true               # Collapses listview until typing starts

# Category Badge Styling ("Script", "Application", "Plugin")
[listview.category_badge]
show = true
color = "$subtle"
font_size_offset = 3.0
radius = 4.0
padding_x = 6.0
border = false

# Alias Pill Badge Styling — renders one pill per [aliases] entry that
# points at a target (e.g. "gh" next to "GitHub"). Add the "alias_badge"
# slot to [element] layout to show it.
[listview.alias_badge]
show = true
color = "$accent"
font_size_offset = 3.0
radius = 4.0
padding_x = 6.0
border = true
border_color = "$accent"
```

### Row Slots & Hover States (`[element]`)
```toml
[element]
padding = [8.0, 12.0]
corner_radius = 8.0
background = "transparent"
text_color = "$text"
description_color = "$subtle"
show_icons = true
icon_size = 22.0

# Customize the slot ordering inside each row!
# Available slots: "icon", "name", "spacer", "category_badge", "alias_badge", "shortcut"
# (plus any custom widget id defined under [widgets.*])
layout = [
    "icon",
    "name",
    "alias_badge",
    "spacer",
    "category_badge",
    "shortcut"
]

# Currently focused/selected row
[element.selected]
background = "$surface2"
text_color = "$text"
description_color = "$accent"

# Mouse hover row
[element.hover]
background = "$surface"
text_color = "$text"
description_color = "$subtle"
```

### Semantic Status Colors
Used for GUI interactive mode items (e.g. clipboard manager or script results):
```toml
[status_colors]
urgent_background = "$urgent"
urgent_text = "#ffffff"
urgent_row_background = "#f7768e22"
active_background = "$green"
active_text = "#1a1b26"
active_row_background = "#9ece6a22"
accent = "$accent"
muted = "$subtle"
```

### Floating Toast
Status notification styling for background and compact script runs:
```toml
[toast]
running_dot = "#aaaaaa"
success_dot = "$green"
error_dot = "$urgent"
```

### Palette Variable Aliases (`[colors]`)
Define reusable color tokens. Any `$variable` in the theme is automatically replaced with its definition:
```toml
[colors]
bg = "#1a1b26f0"        # 8-digit hex supports alpha channel transparency
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

## 🧩 Custom Widgets System

aerofi includes a declarative widget system. You can build custom headers, sidebars, status bars, or action button strips directly in your theme without touching Rust code.

Widgets are defined either using table syntax `[widgets.<id>]` or array syntax `[[widgets]]`.

### Widget Types & Properties

| Type | Description | Key Properties |
|---|---|---|
| **`box`** | Container for grouping widgets | `orientation` ("horizontal" / "vertical"), `gap`, `padding`, `align` ("left", "center", "end"), `background`, `radius`, `width`, `height`, `flex`, `children` |
| **`text`** | Static typography label | `text`, `color`, `font_size`, `font_weight`, `align` |
| **`icon`** | Symbol or emoji | `icon`, `size`, `color` |
| **`image`** | Image asset | `path`, `width`, `height`, `radius` |
| **`spacer`** | Flexible expanding space | Expands horizontally or vertically to push siblings apart |
| **`divider`** | Separator rule | `color`, `thickness`, `margin` |
| **`button`** | Clickable action button | `text`, `icon`, `action` (target name), `hotkey`, `background`, `hover_background`, `color`, `radius`, `padding`, `gap` |

---

### Composing a Custom Header and Footer

Here is a practical example of adding a custom header with a status pill and a footer bar with interactive buttons:

```toml
# 1. Add widgets to mainbox
[mainbox]
orientation = "vertical"
children = [
    "header_bar",
    "InputBar",
    "ListView",
    "footer_bar"
]

# 2. Define Header Container
[widgets.header_bar]
type = "box"
orientation = "horizontal"
gap = 8.0
padding = [4.0, 4.0]
align = "center"
children = ["header_icon", "header_title", "header_spacer", "header_badge"]

[widgets.header_icon]
type = "icon"
icon = "⚡"
size = 14.0
color = "$accent"

[widgets.header_title]
type = "text"
text = "aerofi"
color = "$text"
font_size = 12.0
font_weight = "bold"

[widgets.header_spacer]
type = "spacer"

[widgets.header_badge]
type = "text"
text = "PRO"
color = "$accent"
font_size = 10.0
font_weight = "semibold"

# 3. Define Footer Container
[widgets.footer_bar]
type = "box"
orientation = "horizontal"
gap = 8.0
padding = [8.0, 4.0]
align = "center"
children = ["footer_help", "footer_spacer", "reload_button"]

[widgets.footer_help]
type = "text"
text = "↵ Open • ⎋ Close • ⌘R Reload"
color = "$subtle"
font_size = 11.0

[widgets.footer_spacer]
type = "spacer"

[widgets.reload_button]
type = "button"
text = "Reload Config"
icon = "🔄"
action = "Reload Configuration"
hotkey = "cmd+r"
color = "$text"
background = "$surface"
hover_background = "$surface2"
radius = 6.0
padding = [4.0, 8.0]
font_size = 11.0
gap = 4.0
```

---

## ⚡ Performance Optimization

1. **Keep `max_results` around 20–30**: Ensures near-zero memory allocation during fuzzy filtering.
2. **Use Frosted Glass Blur Judiciously**: Real-time macOS blur is highly optimized on Apple Silicon Metal, but setting `window.blur = false` is available for pure minimum-power setups.
3. **Use `.dylib` Plugins for Large Datasets**: For indexing tens of thousands of items (e.g. Spotlight or database queries), use aerofi's native C ABI `.dylib` plugin system ([docs/plugins.md](plugins.md)) to bypass CLI process spawning.
