#!/usr/bin/env bash
# @aerofi.schemaVersion 1
# @aerofi.title Power Menu
# @aerofi.mode gui
# @aerofi.icon ⏻
# @aerofi.packageName System
# @aerofi.description Sleep, lock, restart, or shut down
# @aerofi.show_search false
# @aerofi.columns 1
# @aerofi.preset list

printf '\0no-custom\x1ftrue\n'
printf 'Sleep\0icon\x1f💤\0id\x1fsleep\n'
printf 'Lock\0icon\x1f🔒\0id\x1flock\n'
printf 'Restart\0icon\x1f🔄\0id\x1frestart\n'
printf 'Shut Down\0icon\x1f⏻\0id\x1fshutdown\n'

read -r choice

case "$choice" in
  sleep)    pmset sleepnow ;;
  lock)     pmset displaysleepnow ;;
  restart)  sudo shutdown -r now ;;
  shutdown) sudo shutdown -h now ;;
esac
