#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Compact Mode Example
# @aerofi.mode compact
# @aerofi.packageName Examples
# @aerofi.icon 
# @aerofi.description Displays a small floating toast showing real-time single-line progress

# Compact mode renders a floating indicator with running status.
# Each line printed to stdout updates the single-line message in the toast.

echo "Step 1/3: Initializing environment..."
sleep 1
echo "Step 2/3: Processing payload..."
sleep 1
echo "Step 3/3: Done! All tasks completed."
