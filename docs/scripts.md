# aerofi Scripting & GUI Protocol Guide

aerofi features an extensible script execution engine. You can write scripts in any programming or scripting language—**Bash, Zsh, Python, Node.js, Ruby, Swift, or compiled Rust**—and execute them directly from your keyboard.

---

## Quick Start: Where Scripts Live

Scripts are indexed from directories specified in `~/.config/aerofi/config.toml`. By default, aerofi scans:

```
~/.config/aerofi/scripts/
```

Any file with executable permissions (`chmod +x`) or a valid shebang line (e.g. `#!/usr/bin/env bash` or `#!/usr/bin/env python3`) is automatically detected and added to the launcher index.

---

## Script Metadata Annotations (`@aerofi.*` & `@raycast.*`)

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
| `schemaVersion` | Metadata schema version (parsed for Raycast compatibility) | `@aerofi.schemaVersion 1` |
| `mode` | Execution mode (see below) | `@aerofi.mode silent` |
| `icon` | Emoji, Nerd Font glyph, or image path (see [Icons](#icons)) | `@aerofi.icon 🚀` |
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
| `preset` | Preset name from the theme's `[presets.*]` tables applied to the script's list (e.g. `list`, `emoji`), overriding the default | `@aerofi.preset list` |

### Icons

The `icon` field (headers and `icon:` row fields) accepts three kinds of values:

| Kind | Example | Rendering |
|---|---|---|
| Emoji | `🚀` | Color, via Apple Color Emoji fallback |
| Nerd Font glyph | literal char, e.g. `U+F0EA` (fa-clipboard) | Monochrome in the theme font, tinted with `icon_color` |
| Image path | `/path/or.png`, `~/img.jpg` | Thumbnail, scaled to `icon_size` |

Nerd Font glyphs (Font Awesome, Material Design Icons, …) live in the
private-use area. They render in the theme font when it is a Nerd Font
variant (the examples use `JetBrainsMono Nerd Font Mono`), and in any other
theme they resolve through aerofi's automatic glyph-fallback cascade, which
tries common Nerd Font families — so installing any
[nerd-fonts](https://www.nerdfonts.com/) build of your favorite font makes
icon glyphs work in every theme, including the default one. If no Nerd Font
is installed at all the glyph shows as a missing-glyph box — text and emoji
are unaffected (the same "install a Nerd Font" state rofi has for
font-based icons). Prefer Nerd Font glyphs for launcher chrome and keep real
emoji for content (e.g. the emoji picker). The `emoji:` prefix
(`emoji:🚀`) is a hint that the glyph is a color emoji and is stripped
before rendering.

---

## Execution Modes

aerofi provides six dedicated execution modes tailored for different workflows. Each mode has a minimalist reference script in `examples/scripts/`:

### 1. `silent`
Runs the script detached in the background. The aerofi search window closes immediately. If the script produces output or errors, a compact floating toast appears in the corner of your screen; a clean no-output exit shows nothing.
- **Example Script**: [examples/scripts/silent.sh](../examples/scripts/silent.sh)
- **Ideal for**: Triggering background automations, toggling system settings, running backup jobs.

### 2. `compact`
Displays a minimalist floating indicator on screen while the script runs, then shows its final output line when it finishes.
- **Example Script**: [examples/scripts/compact.sh](../examples/scripts/compact.sh)
- **Ideal for**: Fast actions that take 1–3 seconds and report brief progress.

### 3. `inline`
The script runs in the background and its output is displayed directly as a subtitle inside the aerofi launcher list.
- **Example Script**: [examples/scripts/inline.sh](../examples/scripts/inline.sh)
- **Ideal for**: Live status widgets (e.g. current Spotify track, active Git branch, weather, battery health). When combined with `@aerofi.refreshTime 10s`, aerofi automatically refreshes the output periodically.
- **Stopping the background refresh**: the refresh daemon lives only while the script is present with `mode: inline` + `refreshTime`. To stop it, either remove the `@aerofi.refreshTime` line, change `@aerofi.mode` to something else, or delete the script — then press `Cmd+R` (Reload Configuration). Aerofi reconciles the daemon set on every reload and kills the retired daemon's whole process group. Quitting aerofi also kills all of them. While the launcher is hidden the daemons are paused (no CPU); they get one fresh tick when you show the window again.

### 4. `fullOutput`
Executes the command and renders stdout in aerofi's built-in rich markdown viewer. Supports headings, tables, code blocks, blockquotes, and lists.
- **Example Script**: [examples/scripts/full-output.sh](../examples/scripts/full-output.sh)
- **Ideal for**: Viewing documentation, API responses, logs, or curl outputs.
- **Output cap**: to keep memory bounded, the first 1 MiB of stdout is kept (plus 256 KiB of stderr). Anything beyond that is dropped and a `… [output truncated: showing first … of …]` note is appended, so huge script outputs can never spike aerofi's RAM.

### 5. `pipe`
Executes the command, captures its stdout, and immediately copies the result to your macOS system clipboard (`pbcopy`).
- **Example Script**: [examples/scripts/pipe.sh](../examples/scripts/pipe.sh)
- **Ideal for**: UUID generators, password generators, timestamp formatters, base64 encoders.
- **Output cap**: up to 8 MiB of stdout is copied; beyond that the copy is truncated with a visible note (an 8 MiB clipboard payload is already far past any sane use).

### 6. `gui`
Enables aerofi's **two-way interactive GUI mode**. Maintains a persistent, bidirectional process pipeline (`stdin`/`stdout`) between aerofi and your script, turning aerofi into a custom interactive UI (like Rofi or dmenu, but with rich styling).
- **Example Script**: [examples/scripts/gui.sh](../examples/scripts/gui.sh)
- **Comprehensive Examples**: [theme_switcher.py](../examples/scripts/theme_switcher.py), [clipboard.py](../examples/scripts/clipboard.py)
- **Multi-Step Guide**: [multi-step-scripts.md](./multi-step-scripts.md) — building interactive chains, the event loop pattern, and how this compares to Rofi

---

## Interactive GUI Mode Protocol

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
| `\0message` | `<text>` | Displays a status hint in a banner below the search bar |
| `\0flush` | none | Frame delimiter: tells aerofi to render buffered items immediately |
| `\0active` | `<indices>` | Marks specific row(s) with the active state (comma-separated indices) |
| `\0data` | `<token>` | Opaque token echoed back to the script on the next event |
| `\0reload` | `true` | Immediately reloads aerofi configuration, theme, and targets |
| `\0columns` | `<number>` | Dynamically sets grid column count (e.g. 1, 2, 4) |
| `\0loading` | `true` \| `false` | Toggles async loading indicator |
| `\0live-search` | `true` \| `false` | Streams search queries to script stdin for server-side search |
| `\0preview` | `<text>` | Updates preview pane with custom text or markdown |
| `\0preview-file`| `<path>` | Displays a file preview in the preview pane |
| `\0no-custom` | `true` \| `false` | Restricts selection to existing items only |
| `\0keep-selection` | `<row text>` | Pre-select (highlight) the row whose text matches on the next frame, instead of resetting to row 0 |

#### 2. Row Items & Metadata
Rows are printed one per line. Metadata fields can be attached using `\0` delimiters:

```
Title Text\0id\x1fitem-1\0icon\x1femoji:🎨\0info\x1fBadge\0meta\x1ffull text search index
```

- `id\x1f<id>`: Unique item identifier returned in selection events.
- `icon\x1f<spec>`: Row icon — a plain emoji, an `emoji:🚀`-prefixed emoji, or an image path (`/abs/path.png`, `~/path.png`, `./path.png`).
- `info\x1f<text>`: Secondary badge text aligned on the right.
- `meta\x1f<text>`: Invisible search string. aerofi's fuzzy matcher indexes this string in addition to the visible row text.
- `nonselectable\x1ftrue`: Makes the row a static header or separator that cannot be focused.
- `urgent\x1ftrue`: Renders the row with the urgent accent color and an `URGENT` badge.
- `active\x1ftrue`: Renders the row with the active background and a native `ACTIVE` badge.
- `disabled\x1ftrue`: Greys out the row and prevents selection.

---

### Pango Markup Styling

When `\0markup-rows\x1ftrue` is enabled, row text and `info` fields support Pango-style tags:

```xml
<span foreground="#7aa2f7" weight="bold">Primary Title</span> <span foreground="#565f89">subtitle</span>
```

Supported tags:
- `<span foreground="#HEX">...</span>`
- `<span foreground="#HEX" weight="bold">...</span>`
- `<span background="#HEX">...</span>` (highlight background behind the text, useful for colour swatches)
- `<b>...</b>` (bold)
- `<i>...</i>` (italic)
- `<s>...</s>` (strikethrough)
- `<u>...</u>` (underline)
- Escaped entities: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`

Hex colours accept 6- or 8-digit (with alpha) values.

---

### Receiving User Events in Script (`stdin`)

When a user interacts with aerofi (presses `Enter`, a contextual key like `Shift+Enter`/`Ctrl+D`, or submits custom text), aerofi sends a structured event line to your script's `stdin`:

```
\0event\x1fselect\x1fkey:enter\x1findex:0\x1fid:row-0\x1ftext:Selected Text\x1fretv:1\x1fids:row-0,row-3\x1ftexts:Selected Text,Other Item\n
```

Event types: `select` (plain `Enter`), `action` (contextual keys: `Shift+Enter`, `Alt+Enter`, `Ctrl+<key>`), `custom` (free-form text submitted while no row is highlighted). Note that `Tab` multi-selection is handled inside aerofi — it never reaches the script; the toggled rows arrive in `ids`/`texts` of the next `select` event.

**Live-search mode:** when `\0live-search\x1ftrue` is set, aerofi also sends a separate `\0change\x1f<query>` line to `stdin` on every keystroke and backspace, carrying the current search text for server-side filtering (in addition to the event lines above).

> [!WARNING]
> **Bash scripts:** event lines begin with a NUL byte (`\0`), and bash variables cannot store NUL — a plain `read -r line` stops at the NUL and returns an empty string. Consume the NUL first, then read the line:
>
> ```bash
> IFS= read -r -n 1 _nul   # discard the leading NUL
> IFS= read -r event_line  # read the rest of the event line
> ```
>
> Python scripts are unaffected: strip it with `line.lstrip("\x00")`.

Fields in the event line (separated by `\x1f`, prefixed with the field name and `:`):
- `key`: Key pressed (`"enter"`, `"shift+enter"`, `"alt+enter"`, `"ctrl+d"`, or `"custom"` — `kb-custom-N` bindings always report `key:custom`, with the binding number encoded in `retv`).
- `index`: 0-based index of the highlighted row in the last emitted frame.
- `id`: Row ID (the `id` field of the highlighted row; empty when absent).
- `text`: Row text content.
- `retv`: Return code (1 = `Enter`, 2 = custom text, 10 = contextual action, 10–28 = `kb-custom-*` bindings).
- `ids`: Comma-separated list of selected item IDs (all toggled rows when multi-selection is active, otherwise the highlighted row).
- `texts`: Comma-separated display texts matching `ids`.
- `data`: Opaque token set with `\0data`, echoed back when present.

---

## Reference Implementations

### 1. Theme Switcher (`theme_switcher.py`)
- [examples/scripts/theme_switcher.py](../examples/scripts/theme_switcher.py)

Uses native `@aerofi.*` metadata tags to build an interactive theme previewer:
- Scans installed themes in `~/.config/aerofi/themes/` plus the built-in default.
- Renders each palette as colour swatches using Pango markup (`<span foreground="#HEX">■</span>`).
- Marks the current theme with the launcher's native `ACTIVE` badge (`active\x1ftrue` row field).
- Pressing `Enter` updates `~/.config/aerofi/config.toml` and triggers `\0reload` for an instant live re-theme.

### 2. Clipboard Manager (`clipboard.py`)
- [examples/scripts/clipboard.py](../examples/scripts/clipboard.py)

This script demonstrates:
- Streaming real-time clipboard history from SQLite.
- Auto-detecting data types (URLs, hex color swatches, JSON, multiline code snippets).
- Real-time Pango syntax highlighting.
- Invisible full-text indexing with `meta\x1f...`.
- Handling `Tab` multi-selection and `Ctrl+D` deletion events.
