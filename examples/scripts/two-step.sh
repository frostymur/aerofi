#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Two-Step Example
# @aerofi.mode rofi
# @aerofi.packageName Examples
# @aerofi.icon 🗂️
# @aerofi.description Pick a category, then an item — Rofi protocol
# @aerofi.show_search false
# @aerofi.columns 1

# Rofi mode is a two-way session: the script emits frames on stdout and aerofi
# sends user events back on stdin. The process stays alive across steps, so a
# script can drive any number of sub-menus without being re-launched.
#
# stdout  ->  \0<command>\x1f<value> control lines and  <text>\0id\x1f...\x1ficon\x1f...
#             data rows, terminated by a \0flush.
# stdin   ->  \0event\x1fselect\x1fkey:...\x1findex:...\x1fid:...\x1ftext:...\x1f...
#             (a leading NUL byte that bash must swallow first)

US=$'\x1f'   # ASCII unit-separator, the protocol's field delimiter

# emit <prompt> <row> [row ...]  — print one frame: a prompt, the rows, a flush.
# Rows use %b so the \0 / \x1f field separators become real NUL / US bytes.
emit() {
    printf '\0prompt\x1f%s\n' "$1"
    shift
    local row
    for row in "$@"; do
        printf '%b\n' "$row"
    done
    printf '\0flush\n'
}

# --- Step 1: choose a category ---------------------------------------------
emit "1/2 · Choose a category" \
    "Networks\0id\x1fnetworks\0icon\x1f📶" \
    "Files\0id\x1ffiles\0icon\x1f📁" \
    "Settings\0id\x1fsettings\0icon\x1f⚙️"

# --- Event loop -------------------------------------------------------------
while :; do
    # aerofi prefixes each event line with a NUL byte; bash can't store NUL,
    # so swallow that single byte first, then read the rest of the line.
    IFS= read -r -n 1 _nul
    IFS= read -r line || break

    # "change\x1f<query>" is a live-search update. This template doesn't filter.
    if [[ "$line" == change* ]]; then continue; fi

    # Split "event\x1fselect\x1f…id:X\x1ftext:Y…" on the unit-separator and pull
    # out the id: and text: fields (order-independent).
    IFS="$US" read -r -a f <<<"$line"
    id="" ; text=""
    for p in "${f[@]}"; do
        case "$p" in
            id:*)   id="${p#id:}" ;;
            text:*) text="${p#text:}" ;;
        esac
    done

    case "$id" in
        networks)
            emit "2/2 · Networks" \
                "wifi-home\0id\x1fwifi_home\0icon\x1f📶" \
                "wifi-guest\0id\x1fwifi_guest\0icon\x1f📶"
            ;;
        files)
            emit "2/2 · Files" \
                "Desktop\0id\x1fdesktop\0icon\x1f🖥️" \
                "Downloads\0id\x1fdownloads\0icon\x1f⬇️"
            ;;
        settings)
            emit "2/2 · Settings" \
                "Appearance\0id\x1fappearance\0icon\x1f🎨" \
                "Shortcuts\0id\x1fshortcuts\0icon\x1f⌨️"
            ;;
        *)
            # A leaf item was chosen — do the real work here, then stop.
            printf 'two-step: picked %q (id=%q)\n' "$text" "$id" >&2
            exit 0
            ;;
    esac
done
