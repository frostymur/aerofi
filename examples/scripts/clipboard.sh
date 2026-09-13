#!/usr/bin/env bash

# @raycast.schemaVersion 1
# @raycast.title Clipboard History
# @raycast.mode gui
# @raycast.packageName System
# @raycast.icon 📋
# @raycast.description Search, preview, and manage clipboard history with clipy
# @aerofi.show_search true
# @aerofi.columns 1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$SCRIPT_DIR/clipboard.py" "$@"
