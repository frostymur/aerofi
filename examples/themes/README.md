# aerofi Themes

This directory contains curated example themes for aerofi.

## Available Themes

### 1. Tokyo Night (`tokyo-night.toml`)
A clean, modern dark theme inspired by the popular Tokyo Night color palette. Features deep indigo surfaces, neon blue/purple accents, and smooth translucent frosted glass.

- **Background**: `#1a1b26` with frosted glass blur
- **Accents**: Neon Cyan (`#7dcfff`), Sky Blue (`#7aa2f7`), Violet (`#bb9af7`)
- **Text**: Crisp `#c0caf5` with dimmed subtle hints

---

### 2. Gruvbox Dark (`gruvbox.toml`)
A warm, vintage retro-groove theme designed for optimal contrast and eye comfort during long coding and typing sessions.

- **Background**: Deep warm charcoal (`#282828`)
- **Accents**: Warm Gold (`#fabd2f`), Terracotta Orange (`#fe8019`), Forest Green (`#b8bb26`)
- **Text**: Warm off-white (`#ebdbb2`)

### 3. Tokyo Night Grid (`tokyo-night-grid.toml`)
A 4-column icon tile grid search layout styled with the Tokyo Night palette.

- **Layout**: 4-column tile grid with large 72pt icons
- **Window**: 51.0% × 46.0% of the screen (screen-relative)

---

### 4. Catppuccin Mocha (`catppuccin-mocha.toml`)
The [Catppuccin Mocha](https://catppuccin.com) palette in a **split two-pane layout**:
a fully transparent left pane (search bar on top) and a solid right pane for the
results list. The window is a frosted-glass blur; the opaque right pane covers it,
so only the left side reads as blurred.

- **Layout**: Two equal panes side by side (search left, results right)
- **Window**: 62.0% of screen width (~20% larger than the 51.7% default), fully transparent + blurred
- **Right pane**: Opaque background (hides the window blur → "no blur" on that side)
- **Accents**: Latte blue (`#89b4fa`), Mauve (`#cba6f7`), Sage green (`#a6e3a1`)

---

### 5. Graphite Mono (`graphite-mono.toml`)
A minimal, strictly **monochrome** graphite palette — no hue, just grayscale
steps — in the same split two-pane layout as Catppuccin.

- **Background**: Near-black graphite (`#18181b`)
- **Surfaces**: `#27272a` / `#3f3f46`
- **Text / Accent**: Off-white `#f4f4f5`, monochrome accent `#e4e4e7`

---

### 6. Modular Themes, Mixins & Widgets
**Every bundled theme is modular** — a thin file that composes reusable mixins via the top-level `imports = [...]` array. The building blocks:

- **`colors/tokyo-night.toml`**: Standalone Tokyo Night palette and status color definitions.
- **`colors/gruvbox.toml`**: Standalone Gruvbox Dark palette and status color definitions.
- **`colors/catppuccin-mocha.toml`**: Standalone Catppuccin Mocha palette, including an opaque `panel` colour for the split layout's right pane.
- **`colors/graphite-mono.toml`**: Standalone monochrome graphite palette and status color definitions.
- **`layouts/compact.toml`**: Reusable single-column list layout geometry and styling.
- **`layouts/grid.toml`**: Reusable 4-column tile grid layout geometry.
- **`layouts/split.toml`**: Reusable two-pane layout — a transparent search pane on the left and a translucent results pane on the right.
- **`layouts/` variants**: Per-theme tweaks — `catppuccin-split.toml`, `graphite-split.toml`, and `gruvbox-compact.toml`.
- **`widgets/header-bar.toml`**: A header bar (logo + brand) as reusable custom widgets.
- **`widgets/footer-bar.toml`**: A footer bar (hint + quick-action buttons) as reusable custom widgets.

> **`layouts/` ≠ presets.** The `layouts/` files are *geometry mixins* (window + `[mainbox]` + element sizing) imported to build a base theme. They are unrelated to the per-script `[presets.*]` overrides. See the [Customization Guide — Presets vs Layouts](../../docs/customization.md#presets-vs-layouts).

---

## Installation & Usage

### 1. Copy Themes to your aerofi configuration

```bash
mkdir -p ~/.config/aerofi/themes
cp -r examples/themes/* ~/.config/aerofi/themes/
```

### 2. Activate a Theme in `~/.config/aerofi/config.toml`

Set the `theme` key to the file basename (without `.toml`):

#### To use Tokyo Night:
```toml
theme = "tokyo-night"
```

#### To use Gruvbox Dark:
```toml
theme = "gruvbox"
```

#### To use Tokyo Night Grid:
```toml
theme = "tokyo-night-grid"
```

#### To use Catppuccin Mocha (split two-pane):
```toml
theme = "catppuccin-mocha"
```

#### To use the built-in default theme:
```toml
theme = "default"
```

### 3. Apply Changes
Restart aerofi or trigger **Reload Configuration** (`Cmd+R` or search in launcher).

---

## Modular Themes & File Splitting (`imports = [...]`)

You can cleanly separate colors, window dimensions, and widget hierarchies into separate files.

### How `imports` works:
1. **Paths**: Paths in `imports = ["..."]` are resolved relative to `~/.config/aerofi/themes/`.
2. **Deep Merging**: Tables (such as `[window]`, `[font]`, `[inputbar]`, `[colors]`) are recursively merged.
3. **Exclusive Override for Arrays**: Non-table values (including arrays like `children` in `[mainbox]` and `layout` in `[element]`) are completely replaced by the importing theme rather than appended. What you write in the final file is exactly what appears on screen.
4. **Order of Precedence**: Imported files are processed in order, with the importing file having the highest priority.
5. **Cycle Detection**: Circular imports are automatically detected and safely skipped.

### Example: Composing a Modular Theme

`~/.config/aerofi/themes/my-custom-theme.toml`:
```toml
name = "My Custom Theme"

# Import reusable color palette and layout mixins
imports = [
    "colors/tokyo-night.toml",
    "layouts/compact.toml",
]

# Override or tweak only what you need:
[window]
width = 680.0

[colors]
accent = "#bb9af7" # Change accent to Tokyo Night violet
```

### Adding a Widget Mixin

Widget mixins define reusable `[widgets.<id>]` blocks. Import one, then reference its
root widget id in `[mainbox] children` (or a Box's `children`):

```toml
name = "Tokyo Night + Header"

imports = [
    "colors/tokyo-night.toml",
    "layouts/compact.toml",
    "widgets/header-bar.toml",
]

# The layout mixin sets children = ["InputBar", "ListView"]; override it to
# slot the imported widget in (arrays are replaced, not merged).
[mainbox]
children = ["header_bar", "InputBar", "ListView"]
```

---

## Theme Anatomy & Customization

aerofi themes are written in standard TOML. Every section is customizable:

```toml
name = "My Custom Theme"
author = "Your Name"

# $-tokens used below resolve from this palette:
[colors]
bg = "#1a1b26"
surface = "#24283b"
surface2 = "#292e42"
text = "#c0caf5"
subtle = "#565f89"
accent = "#7aa2f7"
urgent = "#f7768e"
green = "#9ece6a"
border = "#33467c"

[font]
family = "SF Pro Text"
size = 15.0

[window]
width = 680.0
height = 450.0
padding = 16.0
background = "$bg"
blur = true
background_opacity = 0.95
corner_radius = 16.0
border_width = 1.0
border_color = "$border"

[inputbar]
height = 46.0
background = "$surface"
text_color = "$text"
placeholder = "Type to search…"
placeholder_color = "$subtle"
corner_radius = 10.0
icon = "❯"
icon_color = "$accent"

[element]
background = "transparent"
text_color = "$text"
description_color = "$subtle"

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
```

---

## Complete Theme Reference & Custom Widgets

For a complete reference showcasing **all layout options (`[mainbox]`), element slots (`[element.layout]`), and custom UI widgets (`[widgets.<id>]`)**:
**See [examples/theme.toml](../theme.toml)**

**Read the complete [Customization Guide](../../docs/customization.md)**
