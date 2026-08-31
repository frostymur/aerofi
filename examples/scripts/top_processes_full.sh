#!/usr/bin/env bash
# @raycast.title Top Processes (Full Output)
# @raycast.mode fullOutput
# @raycast.icon 📈
#
# Shows CPU top processes in full window.
set -euo pipefail

echo "=== TOP PROCESSES BY CPU ==="
ps -erco pid,%cpu,comm | head -n 15
