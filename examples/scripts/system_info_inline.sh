#!/usr/bin/env bash
# @raycast.title System Info (Inline)
# @raycast.mode inline
# @raycast.icon ⚡
#
# Shows short inline system details.
set -euo pipefail

echo "macOS $(sw_vers -productVersion) | Uptime: $(uptime | sed 's/.*up \([^,]*\), .*/\1/')"
