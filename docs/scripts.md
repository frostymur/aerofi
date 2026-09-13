# aerofi Scripting & GUI Protocol Guide

aerofi features an extensible script execution engine. You can write scripts in any programming or scripting language—**Bash, Zsh, Python, Node.js, Ruby, Swift, or compiled Rust**—and execute them directly from your keyboard.

---

## 🚀 Quick Start: Where Scripts Live

Scripts are indexed from directories specified in `~/.config/aerofi/config.toml`. By default, aerofi scans:

```
~/.config/aerofi/scripts/
```

Any file with executable permissions (`chmod +x`) or a valid shebang line (e.g. `#!/usr/bin/env bash` or `#!/usr/bin/env python3`) is automatically detected and added to the launcher index.

---

## 🏷️ Script Metadata Annotations (`@aerofi.*` & `@raycast.*`)

aerofi supports both native `@aerofi.*` annotations and `@raycast.*` metadata headers interchangeably. You can write `@aerofi.*` for your native aerofi scripts, or drop existing Raycast scripts into `~/.config/aerofi/scripts/` without modifying their headers!

### Supported Metadata Annotations

Annotations are placed inside comments (`#`) at the top of your script:

```bash
#!/usr/bin/env bash

# Required parameters:
# @aerofi.schemaVersion 1
# @aerofi.title Search GitHub Repos
# @aerofi.mode fullOutput

# Optional parameters:
# @aerofi.icon 🐙
# @aerofi.packageName Developer Tools
# @aerofi.argument1 { "type": "text", "placeholder": "Repository name" }
# @aerofi.needsConfirmation false
# @aerofi.refreshTime 10m
```

> [!TIP]
> Both `@aerofi.<field>` and `@raycast.<field>` are recognized identically. If both are defined, `@aerofi.<field>` takes precedence.

### Metadata Reference

| Annotation (`@aerofi.*` or `@raycast.*`) | Description | Example |
|---|---|---|
| `title` | The display title in the search results | `@aerofi.title Quick Note` |
| `mode` | Execution mode (see below) | `@aerofi.mode silent` |
| `icon` | Emoji or system icon identifier | `@aerofi.icon 🚀` |
| `iconDark` | Optional dark mode icon identifier | `@aerofi.iconDark 🌟` |
| `packageName` | Category/namespace displayed as subtitle | `@aerofi.packageName Git` |
| `argument[1-3]` | Interactive argument prompt specification | `{"type": "text", "placeholder": "URL"}` |
| `refreshTime` | Periodic background refresh interval | `5m`, `1h` (for `inline` mode) |
| `needsConfirmation` | Prompt for confirmation before running | `true` or `false` |
| `description` | Script description | `@aerofi.description Interactive theme preview` |
| `author` | Author name | `@aerofi.author Your Name` |
| `authorURL` | Author website URL | `@aerofi.authorURL https://github.com/...` |
| `show_search` | Toggle search bar in GUI mode (default `true`) | `@aerofi.show_search false` |
| `columns` | Override list column count (default `1`) | `@aerofi.columns 2` |

---

## ⚡ Execution Modes

aerofi provides six dedicated execution modes tailored for different workflows. Each mode has a minimalist reference script in `examples/scripts/`:

### 1. `silent`
Runs the script detached in the background. The aerofi search window closes immediately. A compact floating toast appears in the corner of your screen indicating execution status.
- **Example Script**: [examples/scripts/silent.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/silent.sh)
- **Ideal for**: Triggering background automations, toggling system settings, running backup jobs.

### 2. `compact`
Displays a minimalist floating indicator on screen while the script runs, streaming single-line output updates until finished.
- **Example Script**: [examples/scripts/compact.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/compact.sh)
- **Ideal for**: Fast actions that take 1–3 seconds and report brief progress.

### 3. `inline`
The script runs in the background and its output is displayed directly as a subtitle inside the aerofi launcher list.
- **Example Script**: [examples/scripts/inline.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/inline.sh)
- **Ideal for**: Live status widgets (e.g. current Spotify track, active Git branch, weather, battery health). When combined with `@aerofi.refreshTime 10s`, aerofi automatically refreshes the output periodically.

### 4. `fullOutput`
Executes the command and renders stdout in aerofi's built-in rich markdown and ANSI terminal viewer. Supports headings, syntax-highlighted code blocks, blockquotes, and lists.
- **Example Script**: [examples/scripts/full-output.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/full-output.sh)
- **Ideal for**: Viewing documentation, API responses, logs, or curl outputs.

### 5. `pipe`
Executes the command, captures its stdout, and immediately copies the result to your macOS system clipboard (`pbcopy`).
- **Example Script**: [examples/scripts/pipe.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/pipe.sh)
- **Ideal for**: UUID generators, password generators, timestamp formatters, base64 encoders.

### 6. `gui`
Enables aerofi's **two-way interactive GUI mode**. Maintains a persistent, bidirectional process pipeline (`stdin`/`stdout`) between aerofi and your script, turning aerofi into a custom interactive UI (like Rofi or dmenu, but with rich styling).
- **Example Script**: [examples/scripts/gui.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/gui.sh)
- **Comprehensive Examples**: [theme_switcher.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/theme_switcher.sh), [clipboard.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/clipboard.sh)

---

## 🖥️ Interactive GUI Mode Protocol

When `@raycast.mode gui` is specified, aerofi treats your script as an interactive UI session.

### Bidirectional Flow

```
+----------------+      stdout: commands & items (\0...)       +----------------+
|                | ----------------------------------------->  |                |
|  Your Script   |                                             |  aerofi Window |
| (Python/Bash)  |  <----------------------------------------- |     (GPUI)     |
+----------------+        stdin: user events (\0event...)      +----------------+
```

---

### Outputting from Script to aerofi (`stdout`)

#### 1. Control Commands
Control commands start with `\0` (null byte) and use `\x1f` (unit separator) to separate the command name and its arguments:

```
\0prompt\x1fType to search…\n
\0markup-rows\x1ftrue\n
\0multi-select\x1ftrue\n
\0message\x1fPress Tab to select multiple, Ctrl+D to delete\n
\0flush\n
```

| Command | Argument | Description |
|---|---|---|
| `\0prompt` | `<text>` | Sets the search bar placeholder text |
| `\0markup-rows` | `true` \| `false` | Enables Pango markup formatting in rows |
| `\0multi-select` | `true` \| `false` | Enables item multi-selection with `Tab` |
| `\0message` | `<text>` | Displays a status hint bar at the bottom |
| `\0flush` | none | Frame delimiter: tells aerofi to render buffered items immediately |
| `\0active` | `<index_or_id>` | Highlights a specific item row |
| `\0urgent` | `<index_or_id>` | Marks an item with urgent status color |

#### 2. Row Items & Metadata
Rows are printed one per line. Metadata fields can be attached using `\0` delimiters:

```
Title Text\0icon\x1femoji:🎨\x1finfo\x1fBadge\x1fmeta\x1ffull text search index
```

- `icon\x1f<spec>`: Row icon (`emoji:🚀`, `file:///path/to/icon.png`, or `system-icon:Finder`).
- `info\x1f<text>`: Secondary badge text aligned on the right.
- `meta\x1f<text>`: Invisible search string. aerofi's fuzzy matcher indexes this string in addition to the visible row text.
- `nonselectable\x1ftrue`: Makes the row a static header or separator that cannot be focused.

---

### Pango Markup Styling

When `\0markup-rows\x1ftrue` is enabled, row text supports Pango XML tags:

```xml
<span foreground="#7aa2f7" weight="bold">Primary Title</span> <span foreground="#565f89">subtitle</span>
```

Supported tags:
- `<span foreground="#HEX">...</span>`
- `<span foreground="#HEX" weight="bold">...</span>`
- `<b>...</b>` (bold)
- `<i>...</i>` (italic)
- `<s>...</s>` (strikethrough)
- `<u>...</u>` (underline)
- Escaped entities: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`

---

### Receiving User Events in Script (`stdin`)

When a user interacts with aerofi (presses `Enter`, `Tab`, `Ctrl+D`, or custom keys), aerofi sends a structured event line to your script's `stdin`:

```
\0event\x1fselect\x1fkey=enter\x1findex=0\x1fid=0\x1ftext=Selected Text\x1fretv=0\x1fselected_ids=0,3\n
```

Fields in `GuiEvent`:
- `key`: Key pressed (`"enter"`, `"tab"`, `"custom"`, etc.).
- `index`: 0-based index of the currently highlighted row.
- `id`: Row ID.
- `text`: Row text content.
- `retv`: Return code (0 = Enter, 1 = Alt/Opt+Enter, 10..28 = Custom shortcuts).
- `selected_ids`: Comma-separated list of all selected item IDs (when `multi-select` is active).

---

## 🌟 Reference Implementations

### 1. 🎨 Theme Switcher (`theme_switcher.sh` / `theme_switcher.py`)
- [examples/scripts/theme_switcher.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/theme_switcher.sh)
- [examples/scripts/theme_switcher.py](file:///Users/timuriskakov/projects/aerofi/examples/scripts/theme_switcher.py)

Uses native `@aerofi.*` metadata tags to build an interactive theme previewer:
- Scans installed themes in `~/.config/aerofi/themes/` and built-in themes.
- Renders live visual color swatches using Pango markup (`<span foreground="#HEX">■</span>`).
- Displays a `✓ Active` badge for the current theme.
- Pressing `Enter` updates `~/.config/aerofi/config.toml` and posts a system notification.

### 2. 📋 Clipboard Manager (`clipboard.sh` / `clipboard.py`)
- [examples/scripts/clipboard.sh](file:///Users/timuriskakov/projects/aerofi/examples/scripts/clipboard.sh)
- [examples/scripts/clipboard.py](file:///Users/timuriskakov/projects/aerofi/examples/scripts/clipboard.py)

This script demonstrates:
- Streaming real-time clipboard history from SQLite.
- Auto-detecting data types (URLs, hex color swatches, JSON, multiline code snippets).
- Real-time Pango syntax highlighting.
- Invisible full-text indexing with `meta\x1f...`.
- Handling `Tab` multi-selection and `Ctrl+D` deletion events.
