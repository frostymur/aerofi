#!/usr/bin/env bash

# @aerofi.title Argument Prompt Demo
# @aerofi.mode fullOutput
# @aerofi.icon 🎛️
# @aerofi.packageName Examples
# @aerofi.description Demonstrates text fields, dropdowns, and optional arguments
# @aerofi.argument1 { "type": "text", "placeholder": "Enter a word", "optional": false }
# @aerofi.argument2 { "type": "dropdown", "placeholder": "Format", "data": [{"title": "Uppercase", "value": "upper"}, {"title": "Lowercase", "value": "lower"}] }
# @aerofi.argument3 { "type": "text", "placeholder": "Optional suffix", "optional": true }

WORD="$1"
FORMAT="$2"
SUFFIX="$3"

echo "# Input Received"
echo ""
echo "- **Arg 1 (Text, Required):** \`$WORD\`"
echo "- **Arg 2 (Dropdown):** \`$FORMAT\`"
echo "- **Arg 3 (Optional text):** \`${SUFFIX:-<none>}\`"
echo ""

echo "# Processed Output"
echo ""

if [ "$FORMAT" = "upper" ]; then
  RESULT=$(echo "$WORD" | tr '[:lower:]' '[:upper:]')
else
  RESULT=$(echo "$WORD" | tr '[:upper:]' '[:lower:]')
fi

echo "### \`$RESULT$SUFFIX\`"
