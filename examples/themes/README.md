# aerofi Themes

This directory contains curated example themes for aerofi.

## 🎨 Available Themes

### 1. 🌃 Tokyo Night (`tokyo-night.toml`)
A clean, modern dark theme inspired by the popular Tokyo Night color palette. Features deep indigo surfaces, neon blue/purple accents, and smooth translucent frosted glass.

- **Background**: `#1a1b26` with frosted glass blur
- **Accents**: Neon Cyan (`#7dcfff`), Sky Blue (`#7aa2f7`), Violet (`#bb9af7`)
- **Text**: Crisp `#c0caf5` with dimmed subtle hints

---

### 2. 🍂 Gruvbox Dark (`gruvbox.toml`)
A warm, vintage retro-groove theme designed for optimal contrast and eye comfort during long coding and typing sessions.

- **Background**: Deep warm charcoal (`#282828`)
- **Accents**: Warm Gold (`#fabd2f`), Terracotta Orange (`#fe8019`), Forest Green (`#b8bb26`)
- **Text**: Warm off-white (`#ebdbb2`)

---

## 🚀 Installation & Usage

### 1. Copy Themes to your aerofi configuration

```bash
mkdir -p ~/.config/aerofi/themes
cp examples/themes/*.toml ~/.config/aerofi/themes/
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

#### To use the built-in default theme:
```toml
theme = "default"
```

### 3. Apply Changes
Restart aerofi or trigger **Reload Configuration** (`Cmd+R` or search in launcher).

---

## 🛠️ Theme Anatomy & Customization

aerofi themes are written in standard TOML. Every section is customizable:

```toml
name = "My Custom Theme"
author = "Your Name"

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

---

## 📖 Complete Theme Reference & Custom Widgets

For a complete reference showcasing **all layout options (`[mainbox]`), element slots (`[element.layout]`), and custom UI widgets (`[widgets.<id>]`)**:
👉 **See [reference.toml](file:///Users/timuriskakov/projects/aerofi/examples/themes/reference.toml) / [examples/theme.toml](file:///Users/timuriskakov/projects/aerofi/examples/theme.toml)**
👉 **Read the complete [Customization Guide](../../docs/customization.md)**

