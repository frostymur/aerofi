#!/usr/bin/env bash
# @raycast.title Generate UUID (Pipe to Clipboard)
# @raycast.mode pipe
# @raycast.icon 📋
#
# Generates a new UUID and pipes it directly into the macOS clipboard.
set -euo pipefail

uuidgen | tr '[:upper:]' '[:lower:]' | tr -d '\n'
