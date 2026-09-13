#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Theme Switcher
# @aerofi.mode gui
# @aerofi.packageName aerofi
# @aerofi.icon 🎨
# @aerofi.description Switch and preview themes live in aerofi
# @aerofi.show_search true
# @aerofi.columns 1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$SCRIPT_DIR/theme_switcher.py" "$@"
