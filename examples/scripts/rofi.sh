#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Rofi Mode Example
# @aerofi.mode rofi
# @aerofi.packageName Examples
# @aerofi.icon 
# @aerofi.description Interactive two-way Rofi protocol example
# @aerofi.show_search true
# @aerofi.columns 1

# Rofi mode creates an interactive two-way communication session over stdin/stdout.
# Output control commands starting with \0 to configure the UI.

printf "\0prompt\x1fSelect an option…\n"
printf "\0markup-rows\x1ftrue\n"
printf "\0message\x1fUse arrow keys and press Enter to select\n"

# Output list items: <text>\0icon\x1f<icon>\x1finfo\x1f<badge>\x1fmeta\x1f<search>
printf "<span foreground=\"#7aa2f7\" weight=\"bold\">First Option</span>\0icon\x1femoji:🚀\x1finfo\x1fPrimary\x1fmeta\x1ffirst option\n"
printf "<span foreground=\"#9ece6a\" weight=\"bold\">Second Option</span>\0icon\x1femoji:✨\x1finfo\x1fSecondary\x1fmeta\x1fsecond option\n"
printf "<span foreground=\"#f7768e\" weight=\"bold\">Third Option</span>\0icon\x1femoji:🔥\x1finfo\x1fDanger\x1fmeta\x1fthird option\n"
printf "\0flush\n"

# Read user event from stdin. aerofi prefixes event lines with a NUL
# byte and bash variables cannot hold NUL, so consume it first, then
# read the rest of the line.
IFS= read -r -n 1 _nul
IFS= read -r event_line
# Upon selection, perform action or exit
exit 0
