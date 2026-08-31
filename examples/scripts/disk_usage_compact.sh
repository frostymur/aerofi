#!/usr/bin/env bash
# @raycast.title Disk Free Space (Compact)
# @raycast.mode compact
# @raycast.icon 💾
#
# Shows a small HUD-style block for disk usage.
set -euo pipefail

df -h / | awk 'NR==2 {print "Disk Space: " $4 " free (Used: " $5 ", Total: " $2 ")"}'
