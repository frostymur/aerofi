#!/usr/bin/env bash
# @raycast.title Send Notification (Silent)
# @raycast.mode silent
# @raycast.icon 🔔
#
# Sends a system notification and runs without opening a window.
set -euo pipefail

osascript -e 'display notification "Hello from AeroFi Silent Mode!" with title "AeroFi"'
