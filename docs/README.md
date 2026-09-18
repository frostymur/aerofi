# aerofi Documentation

Index of the aerofi docs. Start here, then jump to the page you need.

## Reference

| Page | What it covers |
|---|---|
| [scripts.md](./scripts.md) | Scripting reference: all six execution modes, `@aerofi.*` / `@raycast.*` metatags, and the complete interactive **GUI wire protocol** (control commands, row fields, events). |
| [customization.md](./customization.md) | Configuration schema, `config.toml`, and the declarative **theming / widgets** system. |

## Guides

| Page | What it covers |
|---|---|
| [multi-step-scripts.md](./multi-step-scripts.md) | Building **interactive multi-step** scripts: the persistent two-way session, the event-loop pattern, flow-control building blocks, and how this compares to Rofi. |
| [plugins.md](./plugins.md) | Native **C ABI `.dylib` plugins**: the plugin API, building, and installing them. |

## Companion examples

- [`examples/scripts/`](../examples/scripts/) — ready-to-run scripts for every mode, including the GUI examples linked from [multi-step-scripts.md](./multi-step-scripts.md).
- [`examples/themes/`](../examples/themes/) — theme, color-palette, and layout examples.
- [`examples/plugins/`](../examples/plugins/) — two reference plugins (Web Search, Spotlight File Search).
