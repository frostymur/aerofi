#!/usr/bin/env bash
# @raycast.title Git Status
# @raycast.mode fullOutput
# @raycast.icon 📊
#
# Shows a short status of the current git repository.
set -euo pipefail

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository."
  exit 1
fi

git status --short --branch
