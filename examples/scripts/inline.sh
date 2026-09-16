#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Inline Mode Example
# @aerofi.mode inline
# @aerofi.packageName Examples
# @aerofi.icon 
# @aerofi.refreshTime 10s
# @aerofi.description Displays output directly as a subtitle in the launcher list

# Inline mode output appears as the subtitle row right beneath the title in aerofi.
# It can automatically refresh at a periodic interval defined by @aerofi.refreshTime.

echo "Status: Active • Time: $(date +'%H:%M:%S') • System: macOS"
