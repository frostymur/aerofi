<div align="center">

# aerofi

**A blazing fast, keyboard-driven application launcher and extensible script runner for macOS, built with GPUI and Rust.**

[![Release](https://img.shields.io/github/v/release/frostymur/aerofi?style=flat-square&color=7aa2f7&label=version)](https://github.com/frostymur/aerofi/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/frostymur/aerofi/ci.yml?branch=main&style=flat-square&label=ci)](https://github.com/frostymur/aerofi/actions)
[![Platform](https://img.shields.io/badge/platform-macOS%2013%2B-lightgrey?style=flat-square&logo=apple)](https://apple.com)
[![Language](https://img.shields.io/badge/language-Rust%202024-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Memory](https://img.shields.io/badge/memory-~40MB%20RSS-brightgreen?style=flat-square)](ARCHITECTURE.md)
[![Latency](https://img.shields.io/badge/latency-%3C2ms-blueviolet?style=flat-square)](ARCHITECTURE.md)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

<br />

[Features](#-key-features) •
[Installation](#-installation) •
[Quick Start](#-quick-start) •
[Customization](docs/customization.md) •
[Scripting](docs/scripts.md) •
[Native Plugins](docs/plugins.md) •
[Architecture](ARCHITECTURE.md)

</div>

---

<div align="center">
<video src="https://github.com/user-attachments/assets/5b45a96a-a9df-49fa-84af-f32730426196" autoplay loop muted playsinline width="820"></video>
  <p><em>Recorded demo showing instant search, theming with custom theme_switcher script, and native file search plugin:</em></p>
</div>

---

## Why aerofi?

Lightweight, open-source macOS launcher compatible with Raycast scripts.
Compare to alternatives:

| Feature | aerofi | Raycast | Alfred |
|---------|--------|---------|--------|
| Memory | ~40 MB | 250 MB | 200 MB |
| Open Source | ✅ | ❌ | ❌ |
| Raycast Scripts | ✅ | ✅ | ❌ |
| C ABI Plugins | ✅ | ❌ | ❌ |
| Cost | Free | $12/mo | $42 |
| Hotkey Latency | <2ms | ~5ms | ~3ms |

## Examples

Ready-to-use scripts in `examples/scripts/`:

- **clipboard-history.sh** — Browse clipboard history
- **calculator.sh** — Quick math calculations
- **emoji-picker.sh** — Search and copy emojis
- **git-branch.sh** — Switch git branches

Copy any of these to `~/.config/aerofi/scripts/` and they work immediately.

## Key Features

**Performance**
- ⚡ Sub-2ms hotkey latency (GPU-accelerated via Metal)
- 🪶 ~40 MB memory (vs 250 MB Raycast)

**Compatibility**
- 📜 Raycast script commands work out-of-the-box
- 🔑 Zero-friction Carbon hotkey (no Accessibility permissions)

**Flexibility**
- 6 execution modes: silent, compact, inline, fullOutput, pipe, gui
- 🎨 Deep TOML theming (colors, fonts, layouts)
- 🔌 C ABI plugins (Rust, C, C++, Swift)

**Developer-Friendly**
- 💻 Open source (MIT license)
- 📚 Full documentation & examples
- 🚀 Active development

## For rofi Users

aerofi is inspired by rofi's Unix philosophy but for macOS:
- Same stdin/stdout piping model (`gui` mode = rofi-compatible)
- Native macOS experience (no X11 layers)
- GPUI rendering (120 FPS, Metal acceleration)
- Modern scripting ecosystem (Raycast compatibility)

You can port rofi scripts to aerofi with minimal changes.

## Installation

### Homebrew (Recommended)
Install aerofi and start it as a native macOS background service:
```bash
brew install frostymur/aerofi/aerofi
brew services start frostymur/aerofi/aerofi
```

### Cargo
```bash
cargo install aerofi
```

### Build from Source
```bash
git clone https://github.com/frostymur/aerofi
cd aerofi
cargo build --release
./target/release/aerofi
```

---

## ⌨️ Default Shortcuts

| Shortcut | Context | Action |
|---|---|---|
| `Option+Space` | Global | Toggle aerofi launcher window |
| `↑` / `↓` (or `Ctrl+P` / `Ctrl+N`) | Launcher | Navigate result list |
| `Enter` | Launcher | Launch highlighted item |
| `Cmd+R` | Launcher | Reload configuration and rescan sources |
| `Escape` | Launcher | Close aerofi window / Clear search |
| `Tab` | GUI Mode | Toggle multi-selection checkbox |
| `Ctrl+D` | GUI Mode | Secondary action (e.g. delete item) |

---

## 🎨 Themes Showcase

aerofi comes bundled with curated modern themes in `~/.config/aerofi/themes/`:

| Theme | Description |
|---|---|
| **Tokyo Night** | Deep indigo surfaces with neon cyan & sky blue accents and frosted glass blur. |
| **Gruvbox Dark** | Warm vintage retro-groove palette with high contrast and earthy tones. |
| **Dark Transparent** | Minimalist semi-translucent dark monochrome design. |

---

## Getting Help

- **Documentation**: [docs/](./docs/) for full config schema and scripting guide
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
