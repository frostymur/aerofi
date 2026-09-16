#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Emoji Picker
# @aerofi.mode gui
# @aerofi.preset grid
# @aerofi.icon 😀
# @aerofi.packageName Fun
# @aerofi.description Pick an emoji from a grid and copy it to your clipboard
# @aerofi.show_search true

# Emoji picker — reads from emoji_data.tsv (emoji<TAB>search terms).
# Selection copies the emoji to the clipboard via pbcopy.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DATA_FILE="$SCRIPT_DIR/emoji_data.tsv"

if [[ ! -f "$DATA_FILE" ]]; then
    printf "\0message\x1femoji_data.tsv not found\n"
    printf "\0flush\n"
    exit 1
fi

printf "\0prompt\x1fSearch emojis…"
printf "\0columns\x1f8\n"
printf "\0message\x1fSelect an emoji to copy it to your clipboard\n"

i=0
while IFS=$'\t' read -r emoji terms; do
    [[ -z "$emoji" ]] && continue
    printf "\0icon\x1f%s\0id\x1fe%s\0meta\x1f%s emoji\n" "$emoji" "$i" "$terms"
    i=$((i + 1))
done < "$DATA_FILE"

printf "\0flush\n"

IFS= read -r -n 1 _nul
IFS= read -r event_line

id=""
IFS=$'\x1f' read -r -a parts <<<"$event_line"
for part in "${parts[@]}"; do
    case "$part" in
    id:*) id="${part#id:}" ;;
    esac
done

if [[ "$id" =~ ^e([0-9]+)$ ]]; then
    idx="${BASH_REMATCH[1]}"
    count=0
    while IFS=$'\t' read -r emoji _; do
        [[ -z "$emoji" ]] && continue
        if [[ "$count" -eq "$idx" ]]; then
            printf "%s" "$emoji" | pbcopy
            break
        fi
        count=$((count + 1))
    done < "$DATA_FILE"
fi

exit 0
