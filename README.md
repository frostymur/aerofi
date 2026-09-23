<div align="center">

# aerofi — Fast macOS Launcher & Raycast Alternative

**A blazing fast, keyboard-driven application launcher and extensible script runner for macOS, built with GPUI and Rust. A lightweight Spotlight, Raycast, and Alfred alternative.**

[![Release](https://img.shields.io/github/v/release/frostymur/aerofi?style=flat-square&color=7aa2f7&label=version)](https://github.com/frostymur/aerofi/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/frostymur/aerofi/ci.yml?branch=main&style=flat-square&label=ci)](https://github.com/frostymur/aerofi/actions)
[![Platform](https://img.shields.io/badge/platform-macOS%2013%2B-lightgrey?style=flat-square&logo=apple)](https://apple.com)
[![Language](https://img.shields.io/badge/language-Rust%202024-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Memory](https://img.shields.io/badge/memory-~45MB%20RSS-brightgreen?style=flat-square)](ARCHITECTURE.md)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

<br />

[Why aerofi?](#why-aerofi) •
[Installation](#installation) •
[Key Features](#key-features) •
[Widgets & Theming](#declarative-widgets--theming) •
[Interactive Scripts](#interactive-gui-scripts--ipc) •
[Shortcuts](#default-shortcuts)

</div>

---

<div align="center">
<video src="https://github.com/user-attachments/assets/5b45a96a-a9df-49fa-84af-f32730426196" autoplay loop muted playsinline width="820"></video>
  <p><em>Recorded demo showing instant search, theming with custom theme_switcher script, and native file search plugin</em></p>
</div>
 
---

## Why aerofi?

**aerofi** is a lightweight, open-source macOS application launcher and extensible productivity tool designed as a fast Spotlight and Raycast alternative. Built in Rust with GPU acceleration (GPUI), it brings fast application launching, a declarative widget & theming engine, six script execution modes (including an interactive stdin/stdout GUI mode), and full Raycast script command compatibility to macOS.

Compare to alternatives:

| Feature | aerofi | Raycast | Alfred |
|---------|--------|---------|--------|
| Memory | ~45-70 MB | 250 MB | ~75 MB |
| Open Source | Yes | No | No |
| Config as Code | Plain TOML (dotfiles) | GUI only | GUI only |
| Declarative Widgets & Theme | TOML | — | — |
| Raycast Scripts | Yes | Yes | No |
| Interactive Script IPC (stdin/stdout) | Yes | No | No |
| C ABI Plugins | Yes | No | No |
| Cost | Free | Free / $12/mo | Free / $42 |

---

## Installation

### Homebrew (Recommended)
Install aerofi and start it as a native macOS background service:
```bash
brew install frostymur/tap/aerofi

# Start as a background service (starts automatically at login)
brew services start aerofi
```

`brew upgrade aerofi` picks up new releases.

### Prebuilt Binary (Direct Download)
Download the latest prebuilt binary from [GitHub Releases](https://github.com/frostymur/aerofi/releases/latest):

- **Apple Silicon (M1–M6)**: [`aerofi-mac-arm64.tar.gz`](https://github.com/frostymur/aerofi/releases/latest/download/aerofi-mac-arm64.tar.gz)
- **Intel Mac (x86_64)**: [`aerofi-mac-x86_64.tar.gz`](https://github.com/frostymur/aerofi/releases/latest/download/aerofi-mac-x86_64.tar.gz)

Unpack and place in your `$PATH` (e.g. `/usr/local/bin` or `~/.local/bin`):
```bash
# Example for Apple Silicon:
curl -L -o aerofi.tar.gz https://github.com/frostymur/aerofi/releases/latest/download/aerofi-mac-arm64.tar.gz
tar -xzf aerofi.tar.gz
sudo mv aerofi /usr/local/bin/

# Run aerofi:
aerofi &
```

### Cargo
```bash
cargo install --git https://github.com/frostymur/aerofi aerofi
```

### Build from Source
```bash
git clone https://github.com/frostymur/aerofi
cd aerofi
cargo build --release
./target/release/aerofi
```

---

## Key Features

**Performance**
- 🪶 ~40 MB memory footprint (vs 250 MB Raycast)

**Compatibility**
- 📜 Raycast script commands work out-of-the-box (`@raycast.*` and `@aerofi.*`)
- 🔑 Zero-friction Carbon hotkey (no Accessibility permissions required)

**Flexibility & Customization**
- 6 execution modes: `silent`, `compact`, `inline`, `fullOutput`, `pipe`, and interactive `gui`
- 🧩 Declarative widget engine (custom headers, footers, action buttons, status pills)
- 🎨 Deep TOML theming (frosted glass blur, fonts, `$palette` tokens, custom layouts)
- 🔌 [Native C ABI plugins](docs/plugins.md) (Rust, C, C++, Swift)

**Developer-Friendly**
- 💻 Open source (MIT license)
- 📚 Full documentation & examples
- 🚀 Active development

---

## Declarative Widgets & Theming

aerofi is completely customizable via transparent, human-readable TOML files in `~/.config/aerofi/`:

- **Declarative Widget System**: Compose custom headers, footers, status pills, and action strips directly in `theme.toml` using `box`, `text`, `icon`, `image`, `spacer`, `divider`, and `button` widgets.
- **Layout Control**: Freely rearrange the UI hierarchy in `[mainbox]` (e.g. place widgets above `InputBar`, between elements, or below `ListView`).
- **macOS Glassmorphism**: Native translucent frosted glass blur, opacity, border radii, shadows, and reusable `$palette` color tokens.

```toml
# Example: Adding a custom header and action footer in theme.toml
[mainbox]
children = ["header_bar", "InputBar", "ListView", "footer_bar"]

[widgets.header_bar]
type = "box"
orientation = "horizontal"
gap = 8.0
children = ["header_title", "spacer", "status_badge"]

[widgets.header_title]
type = "text"
text = "aerofi"
color = "$accent"
font_weight = "bold"
```

### Bundled Themes & Modular Architecture

The built-in **Dark Transparent** theme is always available. Additional curated
themes ship in [`examples/themes/`](examples/themes/) — copy any of them to
`~/.config/aerofi/themes/` and select it with `theme = "<name>"`:

| Theme | Description |
|---|---|
| **Tokyo Night** | Deep indigo surfaces with neon cyan & sky blue accents and frosted glass blur. |
| **Tokyo Night Grid** | Compact 4-column grid layout with larger application icons. |
| **Catppuccin Mocha** | The Catppuccin Mocha palette in a transparent split two-pane layout. |
| **Gruvbox Dark** | Warm vintage retro-groove palette with high contrast and earthy tones. |
| **Graphite Mono** | Minimal strictly-monochrome graphite palette in a split layout. |

Themes support modular splitting via `imports = ["colors/...", "layouts/..."]` to effortlessly mix-and-match color palettes and window layouts.

> Read the [Customization Guide](docs/customization.md) and explore [`examples/themes/`](examples/themes/) for complete references.

---

## Interactive GUI Scripts & IPC

Turn any Bash, Python, Node.js, or Swift script into a dynamic macOS mini-app with `@aerofi.mode gui`:

- **Bidirectional Streaming**: Your script outputs items via `stdout` and receives keyboard events (`Enter`, `Tab`, `Ctrl+D`, custom keys) via `stdin` in real-time.
- **Pango Markup Formatting**: Full support for rich colors, bold text, and badges (`<span foreground="#7aa2f7" weight="bold">Title</span>`).
- **Multi-Selection & Actions**: Interactive multi-select with `Tab`, batch deletion with `Ctrl+D`, and custom keybindings.
- **Instant Live Updates**: Update lists, badges, and search prompts dynamically without restarting the script.

```bash
#!/usr/bin/env bash
# @aerofi.title Theme Switcher
# @aerofi.mode gui
# @aerofi.icon 🎨

echo -e "\0prompt\x1fSelect a theme:\n\0markup-rows\x1ftrue\n\0flush"
echo -e "<b>Tokyo Night</b>\0icon\x1femoji:🌃\0info\x1fActive"
echo -e "<b>Gruvbox Dark</b>\0icon\x1femoji:🌲\0info\x1fCommunity"
```

### Ready-to-Use Scripts

Real-world scripts available in [`examples/scripts/`](examples/scripts/):

- **[`clipboard.py`](examples/scripts/clipboard.py)** — Interactive clipboard manager with syntax highlighting, multi-select (`Tab`), and item deletion (`Ctrl+D`)
- **[`theme_switcher.py`](examples/scripts/theme_switcher.py)** — Interactive GUI theme previewer with live swatches and instant config updating
- **[`full-output.sh`](examples/scripts/full-output.sh)** — Markdown viewer rendering formatted GitHub Flavored Markdown inside the launcher
- **[`compact.sh`](examples/scripts/compact.sh)** — Progress tracking with non-blocking floating toast status indicators
- **[`silent.sh`](examples/scripts/silent.sh)** — Background automation with completion notification toasts

Copy any script to `~/.config/aerofi/scripts/` to use it immediately.

> Read the [Scripting & GUI Protocol Guide](docs/scripts.md) and the [Multi-Step Guide](docs/multi-step-scripts.md), and check out [`examples/scripts/`](examples/scripts/) for working implementations.

---

## For rofi Users

aerofi is inspired by rofi's Unix philosophy but built specifically for macOS:
- Same stdin/stdout piping model (`gui` mode = rofi-compatible)
- Native macOS experience (no X11 layers)
- GPUI rendering (120 FPS, Metal acceleration)
- Modern scripting ecosystem (Raycast compatibility)

You can port rofi scripts to aerofi with minimal changes.

---

## Default Shortcuts

| Shortcut | Context | Action |
|---|---|---|
| `Option+Space` | Global | Toggle aerofi launcher window |
| `↑` / `↓` (or `Ctrl+P` / `Ctrl+N`) | Launcher | Navigate result list |
| `Enter` | Launcher | Launch item (activates a running app) |
| `Shift+Enter` | Launcher | Open a new instance of a running app (`open -n`) |
| `Cmd+R` | Launcher | Reload configuration and rescan sources |
| `Escape` | Launcher | Close aerofi window / Clear search |
| `Tab` | GUI Mode | Toggle multi-selection checkbox |
| `Ctrl+D` | GUI Mode | Secondary action (e.g. delete item) |

**Launching applications.** `Enter` (or clicking a row) activates an app that is
already running, matching macOS `open`. Press `Shift+Enter` to force a new
instance (`open -n`) — handy for apps like Terminal or Finder when you want a
second window. Apps that enforce a single instance may ignore `Shift+Enter`.

---

## Getting Help

- **Documentation**: [docs/](./docs/README.md) — config schema, scripting & GUI protocol, multi-step guides, and plugin development
- **Examples**: [examples/scripts/](./examples/scripts/) for working scripts
- **Issues**: [GitHub Issues](https://github.com/frostymur/aerofi/issues)
- **Discussions**: [GitHub Discussions](https://github.com/frostymur/aerofi/discussions)

## Contributing

We welcome contributions! See [CONTRIBUTING.md](./CONTRIBUTING.md) for guidelines.

## License

MIT — See [LICENSE](./LICENSE)

## Credits

- Built on [GPUI](https://github.com/zed-industries/zed) — the GPU-accelerated UI framework from Zed
- Inspired by [rofi](https://github.com/davatorium/rofi)
- Raycast script compatibility via [@raycast/script-commands](https://github.com/raycast/script-commands)
