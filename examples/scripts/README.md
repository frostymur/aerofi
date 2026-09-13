# aerofi Script Examples

This directory contains ready-to-use and customizable scripts for aerofi.

## 🎨 Theme Switcher (`theme_switcher.sh` / `theme_switcher.py`)

An interactive theme previewer and switcher for aerofi using the bidirectional `gui` protocol and native `@aerofi.*` metadata tags.

### ✨ Highlights
- **Live Color Swatches**: Renders visual palette previews using Pango markup with auto-resolved `$accent` and `$bg` colors.
- **Active Theme Indicator**: Shows a `✓ Active` badge next to your currently selected theme.
- **Automatic Theme Discovery**: Scans `~/.config/aerofi/themes/` and built-in palettes.
- **Instant Activation**: Selecting a theme updates `~/.config/aerofi/config.toml` and sends a desktop notification.

```bash
cp examples/scripts/theme_switcher.sh examples/scripts/theme_switcher.py ~/.config/aerofi/scripts/
chmod +x ~/.config/aerofi/scripts/theme_switcher.*
```

---

## 📋 Clipboard History Manager (`clipboard.sh` / `clipboard.py`)

A glamorous, fully functional clipboard history manager for aerofi powered by the [`clipy`](https://crates.io/crates/clipy) minimal clipboard history CLI and aerofi's interactive `gui` mode.

### ✨ Highlights & Aesthetics

- **Rich Pango Styling**: Auto-detects data types and renders tailored syntax highlights:
  - 🌐 **URLs**: Distinct protocol, highlighted hostname, and dimmed path.
  - 🎨 **Hex Colors**: Colored swatch block (`■ #HEX`) with auto-detected hex color preview.
  - 💻 **Code & CLI Commands**: Syntax tags with command preview and indented second-line preview.
  - 📦 **JSON**: Compact formatted preview with object/array item count badge.
  - ✉️ **Emails**: Highlighted email badge.
  - 📄 **Multiline Text**: Emphasized first line with subtle italic subsequent line snippet (`↵`).
- **Full-Text Fuzzy Search**: aerofi's high-speed fuzzy search engine matches across the entire un-truncated text of every entry using the `meta` attribute.
- **Multi-Selection**: Press `Tab` to select multiple clipboard entries; hitting `Enter` copies all selected snippets combined to the clipboard.
- **Interactive Deletion**: Press `Ctrl+D` (or secondary action) on any entry to immediately delete it from history.
- **Zero-Config Daemon**: Automatically checks and starts the background `clipy watch` daemon if it is not already running.

---

### 🚀 Quick Start & Installation

#### 1. Install `clipy` (if not already installed)

`clipy` is a minimal, blazing fast clipboard history manager written in Rust:

```bash
cargo install clipy
```

#### 2. Install the script into aerofi

Copy `clipboard.sh` and `clipboard.py` into your aerofi scripts directory:

```bash
mkdir -p ~/.config/aerofi/scripts
cp examples/scripts/clipboard.sh examples/scripts/clipboard.py ~/.config/aerofi/scripts/
chmod +x ~/.config/aerofi/scripts/clipboard.sh ~/.config/aerofi/scripts/clipboard.py
```

#### 3. Open aerofi and Launch

1. Press your aerofi hotkey (default: `Option+Space` or configured hotkey).
2. Type `Clipboard History` or `clipboard`.
3. Press `Enter` to open the GUI manager!

---

### ⌨️ Shortcuts & Controls

| Key | Action |
|---|---|
| `Type query` | Real-time fuzzy filter across clipboard entries |
| `Enter` | Copy selected item(s) to system clipboard and close launcher |
| `Tab` | Toggle multi-selection checkbox on items |
| `Ctrl+D` | Delete currently highlighted item from history |
| `Esc` | Exit clipboard manager |

---

### 🛠️ aerofi GUI Protocol Details

The clipboard manager demonstrates several powerful features of the aerofi GUI protocol:

- `# @raycast.mode gui`: Instructs aerofi to maintain an open bidirectional process pipeline (`stdin`/`stdout`).
- `\0prompt\x1f<text>`: Customizes the search bar placeholder text.
- `\0markup-rows\x1ftrue`: Enables inline Pango markup parsing for rich text and colors in list rows.
- `\0multi-select\x1ftrue`: Enables multi-item selection with checkboxes via `Tab`.
- `\0message\x1f<text>`: Displays a status bar hint at the bottom or top of the launcher.
- `\0flush`: Explicit frame delimiter for zero-latency burst rendering.
- `\0event\x1fselect...` and `\0event\x1faction...`: Structured stdin events sent from aerofi back to the script upon user interactions.
