#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Full Output Mode Example
# @aerofi.mode fullOutput
# @aerofi.packageName Examples
# @aerofi.icon 
# @aerofi.description Renders stdout in the built-in rich Markdown viewer

# FullOutput mode opens aerofi's built-in Markdown and ANSI viewer.
# Supports GitHub Flavored Markdown (headings, tables, lists, alerts, code blocks).

cat << 'EOF'
# Full Output Mode

This mode renders formatted **Markdown** directly inside the aerofi window.

## Highlights
- Renders tables, blockquotes, and lists
- Syntax highlights code blocks
- Supports ANSI terminal color escape sequences

### Data Table Example
| Parameter | Type | Default | Description |
|---|---|---|---|
| `mode` | String | `fullOutput` | Script execution behavior |
| `refreshTime` | String | `None` | Auto-refresh interval (e.g. `5m`) |
| `show_search` | Boolean | `true` | Show search bar in Rofi mode |

```json
{
  "status": "success",
  "message": "Hello from aerofi fullOutput mode!"
}
```

> [!TIP]
> Press `Escape` to close or return to the search list.
EOF
