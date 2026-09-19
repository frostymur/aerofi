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
  - [GUI Mode (`[gui]`)](#gui-mode-gui)
  - [Layout Hierarchy (`[mainbox]`)](#layout-hierarchy-mainbox)
  - [Search Bar (`[inputbar]`)](#search-bar-inputbar)
  - [Results List & Badges (`[listview]`)](#results-list--badges-listview)
  - [Row Slots & Selected State (`[element]`)](#row-slots--selected-state-element)
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
family = "SF Pro Text"                                    # Font family name (applies to the whole UI)
size = 15.0                                              # Base size in points
weight = "500"                                           # Weight: a name ("bold", "medium") or a number, 100–900
fallback = ["SF Pro", "SF Mono", "Helvetica Neue", "Arial"] # Fallback glyph fonts
```

`family` is any font installed on your system. To use a font you haven't
installed, drop its `.ttf`/`.otf`/`.ttc` file into `~/.config/aerofi/fonts/`
and restart aerofi — it's registered with the text system at startup and can
be referenced by its real family name. Monospace code blocks in script output
always use a monospace face (`JetBrains Mono`).

Individual elements can override the global font with an optional `font`
sub-table (`[inputbar].font` and `[element].font` — see those sections
below); any field left unset inherits the value above.

For glyphs the theme font lacks, aerofi walks a fallback cascade: the
`fallback` list, then common **Nerd Font** families (JetBrainsMono, Hack,
FiraCode, CascadiaCode, SourceCodePro, Symbols), then Apple Color Emoji and
the system UI font. That's how the monochrome icon glyphs in script rows
render: install any [Nerd Font](https://www.nerdfonts.com/) and they work in
every theme, including the default one. Real emoji always render in color.

> **Memory note:** every font face aerofi loads into the text system — the
> `family`, each entry in `fallback`, and any Nerd Font face pulled in to
> render a PUA icon glyph — is kept resident for the app's lifetime. A Nerd
> Font face is large and typically adds roughly **8–10 MB** to RSS. The cost
> is paid the first time a glyph from that face renders (e.g. the first
> script row with an icon), not at startup. Keeping the `fallback` list short
> and using a single Nerd Font keeps this overhead down.

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

# Position: offset from screen centre in points (0.0 = centred,
# negative = towards the left/top edge).
x_offset = 0.0
y_offset = -40.0

# Optional background image:
# background_image = "~/.config/aerofi/themes/wallpaper.jpg"
# background_position = "cover" # "cover" (default), "left", "right"
```

### GUI Mode (`[gui]`)

GUI mode (theme switcher, script output) renders outside the mainbox
layout. Use `[gui]` to control its appearance independently:

```toml
[gui]
padding = 10.0               # Inset from window edges (default: 0)
```

Useful when `[window].padding = 0` (image pane reaches the edge) but
GUI-mode still needs an inset.

### Per-Mode Element Overrides (`[presets.*]`)

Scripts declare a mode name via `# @aerofi.preset <name>`.
Themes override `[element]` sizes per mode — unset fields inherit:

```toml
# For a script with: # @aerofi.preset emoji
[presets.emoji.element]
padding = [4.0, 4.0]
icon_size = 48.0
corner_radius = 8.0
columns = 8

# For a script with: # @aerofi.preset clipboard
[presets.clipboard.element]
padding = [10.0, 14.0]
```

No `@aerofi.preset` in the script → no overrides applied.

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

# Gap between the mainbox children in points (default: listview.spacing).
# gap = 0.0
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

# Optional font override for the search text (any subset of the
# global [font] fields; unset fields inherit):
# [inputbar.font]
# family = "JetBrains Mono"
# size = 15.0
# weight = "500"                     # name ("bold", …) or number, 100–900
```

### Results List & Badges (`[listview]`)
```toml
[listview]
columns = 1                          # 1 for list; any value >1 renders a grid (2, 3, 4, …)
spacing = 4.0                        # Gap between rows
empty_text = "No matching items"
empty_text_color = "$subtle"
highlight_matches = true             # Highlight the query's matched characters in item names
# match_color = "$green"             # Colour of matched characters (default: status_colors.accent)
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

### Row Slots & Selected State (`[element]`)
```toml
[element]
padding = [8.0, 12.0]
corner_radius = 8.0
background = "transparent"
text_color = "$text"
description_color = "$subtle"
show_icons = true
icon_size = 22.0
icon_gap = 6.0        # gap between the icon and name in a grid cell
border_width = 0.0    # item border width (default: borderless tile)
icon_radius = 4.0     # corner radius applied to the item's icon

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

# Optional font override for row names (any subset of the global [font]
# fields; unset fields inherit):
# [element.font]
# family = "JetBrains Mono"
# size = 15.0
# weight = "bold"
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
| **`text`** | Static typography label | `text`, `color`, `font_size`, `font_weight` (name or number 100–900), `align` |
| **`icon`** | Symbol or emoji | `icon`, `size`, `color` |
| **`image`** | Image asset | `path`, `width`, `height`, `radius` |
| **`spacer`** | Flexible expanding space | Expands horizontally or vertically to push siblings apart |
| **`divider`** | Separator rule | `color`, `thickness`, `margin` |
| **`button`** | Clickable action button | `text`, `icon`, `action` (target name), `hotkey`, `close` (default `true`; set `false` to keep the launcher open after the action), `background`, `hover_background`, `color`, `radius`, `padding`, `gap` |

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

> [!TIP]
> By default a button **closes** the launcher after running its action (`close` defaults to `true`). Set `close = false` on a button to keep the launcher open — handy for actions like toggling a widget or copying, where you want to stay put. It applies to both clicks and the button's `hotkey`.

---

## ⚡ Performance Optimization

1. **Keep `max_results` around 20–30**: Ensures near-zero memory allocation during fuzzy filtering.
2. **Use Frosted Glass Blur Judiciously**: Real-time macOS blur is highly optimized on Apple Silicon Metal, but setting `window.blur = false` is available for pure minimum-power setups.
3. **Power-user option: `.dylib` Plugins for Large Datasets**: For advanced integrations that index tens of thousands of items (e.g. Spotlight or database queries), aerofi provides a native C ABI `.dylib` plugin system ([docs/plugins.md](plugins.md)). Plugins are built from source and placed in `~/.config/aerofi/plugins/`; the example plugins in `examples/plugins/` serve as API demos.
