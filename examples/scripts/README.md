# aerofi Script Examples

This directory contains ready-to-use and reference scripts for aerofi.

---

## Execution Mode Examples

Clean, minimal examples demonstrating all 6 aerofi script execution modes:

| Script | Mode | Description | Behavior |
|---|---|---|---|
| **[`silent.sh`](silent.sh)** | `silent` | Detached background job | Launcher closes immediately; a toast appears only if the script outputs something or errors. |
| **[`compact.sh`](compact.sh)** | `compact` | Progress updates | Floating toast shows a running indicator, then the script's final output line. |
| **[`inline.sh`](inline.sh)** | `inline` | Subtitle widget in list | Prints status directly into launcher row subtitle; auto-refreshes via `@aerofi.refreshTime`. |
| **[`full-output.sh`](full-output.sh)** | `fullOutput` | Rich Markdown reader | Renders formatted Markdown (headings, code blocks, lists, blockquotes). |
| **[`pipe.sh`](pipe.sh)** | `pipe` | Clipboard pipe | Captures stdout and automatically copies it to the macOS clipboard. |
| **[`rofi.sh`](rofi.sh)** | `rofi` | Interactive UI | Bidirectional Rofi-compatible IPC session over stdin/stdout with live Pango markup. |

---

## Interactive Rofi Examples

Richer multi-purpose `rofi`-mode apps, beyond the minimal [`rofi.sh`](rofi.sh) mode demo:

| Script | Description |
|---|---|
| **[`emoji-picker.sh`](emoji-picker.sh)** | Pick an emoji from a tile grid and copy it to the clipboard (reads `emoji_data.tsv`). |
| **[`power-menu.sh`](power-menu.sh)** | Sleep, lock, log out, restart, or shut down — a Nerd-Font power menu. |
| **[`two-step.sh`](two-step.sh)** | Pick a category, then an item — a minimal multi-step sub-menu demo (see the [multi-step guide](../../docs/multi-step-scripts.md)). |
| **[`quicklinks.py`](quicklinks.py)** | A personal quick-links list in `~/.config/aerofi/quicklinks.json`; type a URL to add one. |

---

## Theme Switcher (`theme_switcher.py`)

An interactive theme previewer and switcher for aerofi using the bidirectional `rofi` protocol and native `@aerofi.*` metadata tags.

### Highlights
- **Live Color Swatches**: Renders each palette (bg / surface / text / accent) as colour swatches via Pango markup, with `$alias` references resolved from the theme's `[colors]` table.
- **Active Theme First**: The currently active theme is sorted to the top of the list.
- **Automatic Theme Discovery**: Scans `~/.config/aerofi/themes/*.toml` plus the built-in default.
- **Instant Activation**: Selecting a theme updates `~/.config/aerofi/config.toml` and sends `\0reload` so the launcher re-themes live.

```bash
cp examples/scripts/theme_switcher.py ~/.config/aerofi/scripts/
chmod +x ~/.config/aerofi/scripts/theme_switcher.py
```

---

## Clipboard History Manager (`clipboard.py`)

A glamorous, fully functional clipboard history manager for aerofi powered by the [`clipy`](https://crates.io/crates/clipy) minimal clipboard history CLI, the [clippy suite](https://github.com/neilberkman/clippy) for image support (`brew install clippy`), and aerofi's interactive `rofi` mode.

### Highlights & Aesthetics

- **Rich Pango Styling**: Auto-detects data types and renders tailored syntax highlights:
  - 🌐 **URLs**: Distinct protocol, highlighted hostname, and dimmed path.
  - 🎨 **Hex Colors**: Colored swatch block (`■ #HEX`) with auto-detected hex color preview.
  - 💻 **Code & CLI Commands**: Syntax tags with command preview and indented second-line preview.
  - 📦 **JSON**: Compact formatted preview with object/array item count badge.
  - ✉️ **Emails**: Highlighted email badge.
  - 📄 **Multiline Text**: Emphasized first line with subtle italic subsequent line snippet (`↵`).
- **Image History**: Images copied to the pasteboard are captured on demand (`pasty`), stored as PNG in `~/.config/aerofi/clipboard-history/`, deduplicated by content hash, and pruned to the 50 most recent. Rows show a thumbnail preview and dimensions; selecting one puts the image back on the clipboard (`clippy`), `Ctrl+D` deletes the file. Without the clippy suite the script degrades to text-only.
- **Full-Text Fuzzy Search**: aerofi's high-speed fuzzy search engine matches across the entire un-truncated text of every entry using the `meta` attribute.
- **Multi-Selection**: Press `Tab` to select multiple clipboard entries; hitting `Enter` copies all selected snippets combined to the clipboard.
- **Interactive Deletion**: Press `Ctrl+D` (or secondary action) on any entry to immediately delete it from history.
- **Zero-Config Daemon**: Automatically checks and starts the background `clipy watch` daemon if it is not already running.

---

### Quick Start & Installation

#### 1. Install `clipy` (if not already installed)

`clipy` is a minimal, blazing fast clipboard history manager written in Rust:

```bash
cargo install clipy
```

#### 2. Install the script into aerofi

Copy `clipboard.py` into your aerofi scripts directory:

```bash
mkdir -p ~/.config/aerofi/scripts
cp examples/scripts/clipboard.py ~/.config/aerofi/scripts/
chmod +x ~/.config/aerofi/scripts/clipboard.py
```

#### 3. Open aerofi and Launch

1. Press your aerofi hotkey (default: `Option+Space` or configured hotkey).
2. Type `Clipboard History` or `clipboard`.
3. Press `Enter` to open the Rofi manager!

---

### Shortcuts & Controls

| Key | Action |
|---|---|
| `Type query` | Real-time fuzzy filter across clipboard entries |
| `Enter` | Copy selected item(s) to system clipboard and exit to the search list |
| `Tab` | Toggle multi-selection checkbox on items |
| `Ctrl+D` | Delete currently highlighted item from history |
| `Esc` | Exit clipboard manager |

---

### aerofi Rofi Protocol Details

The clipboard manager demonstrates several powerful features of the aerofi Rofi protocol:

- `# @raycast.mode rofi`: Instructs aerofi to maintain an open bidirectional process pipeline (`stdin`/`stdout`).
- `\0prompt\x1f<text>`: Customizes the search bar placeholder text.
- `\0markup-rows\x1ftrue`: Enables inline Pango markup parsing for rich text and colors in list rows.
- `\0multi-select\x1ftrue`: Enables multi-item selection with checkboxes via `Tab`.
- `\0message\x1f<text>`: Displays a status hint in a banner just below the search bar.
- `\0flush`: Explicit frame delimiter for zero-latency burst rendering.
- `\0event\x1fselect...` and `\0event\x1faction...`: Structured stdin events sent from aerofi back to the script upon user interactions.
