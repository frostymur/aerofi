#!/usr/bin/env bash

# @aerofi.schemaVersion 1
# @aerofi.title Emoji Picker
# @aerofi.mode gui
# @aerofi.icon 😀
# @aerofi.packageName Fun
# @aerofi.description Pick an emoji from a grid and copy it to your clipboard
# @aerofi.show_search true

# Emoji picker — a grid of emoji icons (no text labels). Search filters by
# name/synonyms via the invisible meta field; selection copies the emoji
# to the clipboard through pbcopy.

# entries: "emoji:search terms"
EMOJIS=(
    "😀:grinning face smile happy grin"
    "😂:tears of joy laugh lol cry funny"
    "😍:heart eyes love crush adore"
    "🤔:thinking face hmm wonder ponder"
    "😎:sunglasses cool deal relaxed"
    "🥺:pleading face puppy eyes please sad"
    "🥳:partying face celebrate party hat"
    "🤓:nerd face geek smart studious"
    "😴:sleeping face tired zzz rest"
    "🤯:exploding head mind blown wow"
    "😭:loudly crying sob cry sad"
    "😅:relieved face sweat phew nervous"
    "👍:thumbs up like yes good approve"
    "👎:thumbs down no bad dislike"
    "👋:waving hand hello hi bye wave"
    "🤝:handshake deal agreement partnership"
    "✌️:victory hand peace v sign"
    "🙌:raised hands yay praise celebration"
    "👀:eyes look watch see"
    "🔥:fire hot flame lit trending"
    "💯:hundred perfect score awesome"
    "⭐:star favorite rate shiny"
    "✨:sparkles magic shine glow"
    "💥:collision boom explosion"
    "💻:laptop computer mac dev coding"
    "📱:phone mobile smartphone call"
    "⌨️:keyboard type code input"
    "🎵:music note song audio beat"
    "📷:camera photo picture snap"
    "🔑:key password access unlock"
    "💡:lightbulb idea tip bright"
    "🎯:target bullseye goal focus direct hit"
    "🚀:rocket launch ship deploy fast"
    "❤️:red heart love like"
    "💜:purple heart love violet"
    "💚:green heart love nature"
    "✅:check mark done complete yes ok"
    "❌:cross mark no wrong cancel x"
    "⚠️:warning caution alert danger"
    "❗:exclamation important urgent alert"
    "❓:question mark ask help unsure"
    "💤:zzz sleep tired snooze"
    "☕:coffee tea drink hot cup"
    "🍕:pizza food hungry slice"
    "🎮:video game controller play game"
    "📌:pushpin pin note remember"
    "🔒:lock secure private locked"
    "🌙:crescent moon night sleep dark"
    "🌈:rainbow colors weather prism"
)

printf "\0prompt\x1fSearch emojis…\n"
printf "\0columns\x1f4\n"
printf "\0message\x1fSelect an emoji to copy it to your clipboard\n"

i=0
for entry in "${EMOJIS[@]}"; do
    emoji="${entry%%:*}"
    meta="${entry#*:}"
    printf "\0icon\x1f%s\0id\x1fe%s\0meta\x1f%s emoji\n" "$emoji" "$i" "$meta"
    i=$((i + 1))
done

printf "\0flush\n"

# Read the selection event. aerofi prefixes event lines with a NUL byte and
# bash variables cannot hold NUL, so consume it first, then read the line.
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
    if ((idx < ${#EMOJIS[@]})); then
        printf "%s" "${EMOJIS[idx]%%:*}" | pbcopy
    fi
fi

exit 0
