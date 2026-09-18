#!/usr/bin/env bash
# @aerofi.schemaVersion 1
# @aerofi.title Power Menu
# @aerofi.mode gui
# @aerofi.icon ⏻
# @aerofi.packageName System
# @aerofi.description Sleep, lock, log out, restart, or shut down
# @aerofi.show_search false
# @aerofi.columns 5
# @aerofi.preset power
# @aerofi.width 680

# Each row: <label>\0icon\x1f<glyph>\0id\x1f<id>
# The glyph lives in the icon field (rendered large); the label is the text.
printf '\0no-custom\x1ftrue\n'
printf 'Lock\0icon\x1f󰌾\0id\x1flock\n'
printf 'Sleep\0icon\x1f󰤄\0id\x1fsleep\n'
printf 'Log Out\0icon\x1f󰍃\0id\x1flogout\n'
printf 'Restart\0icon\x1f󰜉\0id\x1frestart\n'
printf 'Shut Down\0icon\x1f⏻\0id\x1fshutdown\n'
printf '\0flush\n'

# aerofi writes the selection back as a structured event line:
#   \0event\x1fselect\x1fkey:enter\x1findex:N\x1fid:<id>\x1ftext:<text>…
# Bash cannot hold a NUL byte, so consume the leading NUL first, then read
# the rest of the line and pull out the selected row's id.
IFS= read -r -n 1 _nul
IFS= read -r event_line

choice=""
IFS=$'\x1f' read -r -a fields <<< "$event_line"
for kv in "${fields[@]}"; do
  case "$kv" in
    id:*) choice=${kv#id:} ;;
  esac
done

case "$choice" in
  lock)     pmset displaysleepnow ;;
  sleep)    pmset sleepnow ;;
  logout)   osascript -e 'tell application "loginwindow" to eject' ;;
  restart)  sudo shutdown -r now ;;
  shutdown) sudo shutdown -h now ;;
esac
