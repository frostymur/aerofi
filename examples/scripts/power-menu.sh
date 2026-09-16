#!/usr/bin/env bash
# @aerofi.schemaVersion 1
# @aerofi.title Power Menu
# @aerofi.mode gui
# @aerofi.icon ⏻
# @aerofi.packageName System
# @aerofi.description Sleep, lock, restart, or shut down
# @aerofi.show_search false
# @aerofi.columns 5
# @aerofi.preset grid
# @aerofi.width 420

printf '\0no-custom\x1ftrue\n'
printf '󰌾\0id\x1flock\n'
printf '󰤄\0id\x1fsleep\n'
printf '󰜉\0id\x1frestart\n'
printf '⏻\0id\x1fshutdown\n'

read -r choice

case "$choice" in
  󰌾)     pmset displaysleepnow ;;
  󰤄)    pmset sleepnow ;;
  󰜉)  sudo shutdown -r now ;;
  ⏻) sudo shutdown -h now ;;
esac
