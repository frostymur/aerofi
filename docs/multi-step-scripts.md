# Multi-Step Scripts in aerofi

A *multi-step* script drives a sequence of screens — pick a category, then an
item, then an action, then confirm — all from one launcher entry. aerofi
supports two mechanisms for this, from lightest to richest:

| Mechanism | Opt-in | Steps | Process |
|---|---|---|---|
| **Argument prompt** | `@aerofi.argumentN` | gather N text inputs, then run | one-shot |
| **Confirmation** | `@aerofi.needsConfirmation` | one yes/no gate before running | one-shot |
| **Rofi session** | `@aerofi.mode rofi` | unlimited interactive sub-screens | persistent, two-way |

The first two are one-shot: aerofi prompts, you type/confirm, then the script
runs once. The **Rofi session** is the real multi-step engine — a long-lived,
bidirectional process where *your script owns the UI across any number of
steps*. The rest of this doc is about that, and how it differs from Rofi.

The full wire protocol (every control command, row field, and event field) is
documented in [scripts.md](./scripts.md#-interactive-rofi-mode-protocol). This
page focuses on the *pattern* for building multi-step flows.

---

## The core model: one persistent, two-way process

When a `rofi`-mode script is executed, aerofi does **not** run it once and read
its output. It spawns the script as a **long-lived child process** with piped
`stdin` and `stdout` and keeps talking to it until the script exits or the
window closes:

- `src/core/rofi_session.rs` — `RofiSession` owns the child (`Command` with
  `stdin`/`stdout` piped). A background thread reads stdout line-by-line and
  forwards it; `send_event()` writes a user event to the script's stdin. `Drop`
  (and window hide) kills the child.
- `src/ui/launcher/state.rs:1214` — `start_rofi_session()` calls
  `RofiSession::spawn()` and transitions the launcher into `RofiMode`.
- `src/core/rofi_protocol.rs` — parses what the script emits (control commands
  + rows) and formats what aerofi sends back (events).

```
                 stdout: frames (control cmds + rows + \0flush)
  +-------------+  -------------------------------------------->  +------------+
  |             |                                                  |            |
  |   your      |  <------------------------------------------------- aerofi    |
  |   script    |   stdin: user events (\0event\x1fselect\x1f... )   |  window    |
  |  (alive)    |                                                  |            |
  +-------------+                                                  +------------+
```

Because the process stays alive, a script can be an **event loop**: emit a
frame, wait for the next selection, branch, emit the next frame, repeat. That
loop *is* the multi-step UI — no re-launch, no process per step.

---

## Anatomy of one step

A step has two halves.

### 1. Emit a frame (stdout)

A frame is a prompt, zero or more rows, and a flush:

```bash
printf '\0prompt\x1fStep 1 · Choose a category\n'
printf '%b\n' "Networks\0id\x1fnetworks\0icon\x1f📶"
printf '%b\n' "Files\0id\x1ffiles\0icon\x1f📁"
printf '\0flush\n'
```

Field rules (see [scripts.md](./scripts.md#2-row-items--metadata) for the full
set):

- **A NUL (`\0`) separates fields; a unit-separator (`\x1f`) separates a
  field's key from its value.** The text before the first `\0` is the display
  text; after it come `id`, `icon`, `info`, `meta`, `nonselectable`, …
- Give every selectable row an `id`. You route the next step off the `id`,
  which is stable even if the display text changes or contains spaces.
- Rows must be printed so `\0` and `\x1f` become **real bytes** — use
  `printf '%b'` (or put the row in the format string). Plain `echo "a\0b"`
  leaves the backslashes literal and the parser sees no fields.

### 2. Receive the selection (stdin)

On `Enter`/action, aerofi writes one event line to your stdin:

```
\0event\x1fselect\x1fkey:enter\x1findex:0\x1fid:networks\x1ftext:Networks\x1fretv:1\x1fids:networks\x1ftexts:Networks
```

- `select` = plain `Enter`; `action` = a contextual key (`Shift+Enter`,
  `Ctrl+D`, …); `custom` = free text submitted with no row highlighted.
  Live-search (when `\0live-search\x1ftrue`) additionally sends
  `\0change\x1f<query>` on every keystroke.
- Fields: `key`, `index`, `id`, `text`, `retv` (1=Enter, 2=custom, 10=action,
  10–28 = `kb-custom-*`), `ids`/`texts` (all rows when Tab multi-select is
  active), optional `data` (echo of `\0data`).

**Bash gotcha:** the line starts with a NUL byte and bash variables cannot hold
NUL. Swallow it first, then read the line:

```bash
IFS= read -r -n 1 _nul      # discard the leading NUL
IFS= read -r event_line     # the rest of the event line
```

Then split on the unit-separator and pull out the `id` you need:

```bash
US=$'\x1f'
IFS="$US" read -r -a f <<<"$event_line"
for p in "${f[@]}"; do
  case "$p" in id:*) id="${p#id:}" ;; esac
done
```

(From Python, just `line.lstrip("\x00")` and split on `"\x1f`.)

---

## Worked example: two steps in bash

[examples/scripts/two-step.sh](../examples/scripts/two-step.sh) is the minimal
multi-step template — *pick a category, then an item, then act* — and shows the
whole loop:

```bash
#!/usr/bin/env bash
# @aerofi.title Two-Step Example
# @aerofi.mode rofi
# @aerofi.show_search false

US=$'\x1f'

emit() {                              # emit <prompt> <row> [row ...]
    printf '\0prompt\x1f%s\n' "$1"
    shift
    local row
    for row in "$@"; do printf '%b\n' "$row"; done
    printf '\0flush\n'
}

# Step 1
emit "1/2 · Choose a category" \
    "Networks\0id\x1fnetworks\0icon\x1f📶" \
    "Files\0id\x1ffiles\0icon\x1f📁"

while :; do
    IFS= read -r -n 1 _nul
    IFS= read -r line || break
    [[ "$line" == change* ]] && continue          # ignore live-search

    IFS="$US" read -r -a f <<<"$line"
    id=""
    for p in "${f[@]}"; do case "$p" in id:*) id="${p#id:}";; esac; done

    case "$id" in
        networks)                                  # Step 2 for this branch
            emit "2/2 · Networks" \
                "wifi-home\0id\x1fwifi_home\0icon\x1f📶" \
                "wifi-guest\0id\x1fwifi_guest\0icon\x1f📶"
            ;;
        *)                                         # a leaf was chosen -> act
            printf 'picked %q\n' "$id" >&2
            exit 0
            ;;
    esac
done
```

Notes that make it actually work:

- **One process, many steps.** The `while :; do … read …; case …; done` loop
  *is* the multi-step machine. Each `case` branch either emits the next frame
  (falling back into `read` for the next selection) or does the final work and
  `exit`s.
- **Route on `id`, not text.** `id:wifi_home` is stable; the display text can
  carry spaces/emoji and still be safe.
- **End the session by exiting.** `exit 0` closes the session (aerofi sees
  stdout EOF). Hiding the window also kills the child, so never `block` on a
  read past the last step.

Richer examples that loop many steps with live data:
[theme_switcher.py](../examples/scripts/theme_switcher.py) (live re-theme via
`\0reload`) and [clipboard.py](../examples/scripts/clipboard.py) (Tab
multi-select + `Ctrl+D` delete events).

---

## Controlling the flow (building blocks)

The control commands in [scripts.md](./scripts.md#1-control-commands) are the
levers for a multi-step UI:

- `\0no-custom\x1ftrue` — only the listed rows are selectable (blocks free-text
  `custom` events). Use it on every "pick one of these" step.
- `\0live-search\x1ftrue` — aerofi streams `\0change\x1f<query>` to your stdin
  for server-side filtering (re-emit a filtered frame in response).
- `\0multi-select\x1ftrue` — Tab/Shift+Tab toggles rows; they arrive in
  `ids`/`texts` on the next `select`.
- `\0columns\x1f3` — switch the layout to a grid for a given step.
- `\0loading\x1ftrue` — show a spinner while you compute the next frame.
- `\0data\x1f<token>` — attach an opaque token that comes back on the next
  event (useful for carrying per-step state without parsing text).
- `\0reload` — ask aerofi to re-read config/theme/targets in place.

### The simpler primitives (when you don't need a session)

If "multi-step" just means *ask for a value, then run*, you don't need a Rofi
session at all:

- `@aerofi.argument1 { "type": "text", "placeholder": "Repo" }` (…up to
  `argument3`) — aerofi shows an inline input bar for each, then runs the
  script with the values as arguments. (state.rs `ArgumentInput`.)
- `@aerofi.needsConfirmation true` — a yes/no gate before the script runs
  (state.rs `Confirming`).

These are one-shot; reach for the Rofi session when you need more than
"collect inputs → run once".

---

## How this compares to Rofi

aerofi's Rofi protocol is deliberately *Rofi-shaped* — same "script owns the
menu, selections flow back" idea and the same `\0`-prefixed / `\x1f`-delimited
control-line convention. The two diverge on **process model**, which is the
whole ballgame for multi-step work.

### Rofi: one-shot, process-per-step

Rofi is a one-shot menu. A *mode* (e.g. a custom mode defined with
`-modi name:command`) runs a command **once** to produce the item list (one
item per line on stdout). You select an item and Rofi runs that mode's *action
command*, passing the selection through argument substitution (`-format` with
`{t}`/`{row}`) or, in `-dmenu` mode, to the command's stdin.

There is **no long-lived interactive session** in stock Rofi. A step's list is
computed once, the menu is static while it's open, and the selection is handed
to a *separate* command. So "multi-step" in Rofi means **chaining separate
`rofi` invocations**:

```bash
# pick a directory, THEN a file — two rofi processes, back to back
dir=$(printf '%s\n' ~/Downloads ~/Documents | rofi -dmenu -p "Dir")
file=$(ls -1 "$dir" | rofi -dmenu -p "File")
open "$dir/$file"
```

The state between steps lives in the shell (`$dir`), not in the menu process.
Each step pays a fresh process start, and there is no channel to *update* an
open menu without closing and relaunching it.

### aerofi: one persistent, event-driven process

aerofi keeps a single child alive and *talks* to it. The same directory→file
flow is one script with two frames in a loop — no second process, and the
script can react to selections, filter live, and keep state in its own
variables:

```bash
# one process, two frames (full version: examples/scripts/two-step.sh)
emit "Dir:" "~/Downloads\0id\x1f~/Downloads" ~/Documents\0id\x1f~/Documents
read event; dir_from_id
emit "File:"   # rows generated from `ls "$dir"`, with size badges
read event; open "$dir/$file"
```

### Side-by-side

| | Rofi | aerofi Rofi mode |
|---|---|---|
| Process model | one process **per step** | one persistent process |
| How a step's list is built | command run once per step | script emits frames on demand |
| Selection handed back | argv / stdin to the *action* command | `select`/`action` event on the same process's stdin |
| Multi-step mechanism | chain separate `rofi` invocations | the script's own event loop |
| State between steps | shell variables / env / files | in-process (script memory) |
| Live filtering of an open menu | re-run the command (new menu) | `\0change\x1f<query>` to the live process |
| Multi-select then act | mode-specific / manual | built-in Tab; rows arrive in `ids`/`texts` |
| Loading spinner, preview pane, dynamic grid | not available | `\0loading`, `\0preview`, `\0columns` |
| Per-step layout | fixed per invocation | `columns`/`active`/`disabled` per frame |

### Translating a Rofi multi-step to aerofi

1. **Collapse the chain into one process.** Each `rofi` call becomes a `frame`
   (a prompt + rows + `\0flush`) in your script's loop.
2. **Replace "selection → next command" with "selection → next frame."** Read
   the `select` event, branch on `id`, emit the next frame instead of
   launching another menu.
3. **Move inter-step state into the script.** The `$dir` you'd pass between two
   rofi calls just becomes a shell variable that survives the loop.
4. **Upgrade where it's free.** Rofi can't do it live; aerofi can: stream
   `change` events to filter a list without relaunching, Tab multi-select,
   `\0loading` while you compute, and `\0preview` for a side pane.

The practical upshot: anything you can build as a chain of Rofi menus you can
build as a single aerofi Rofi script — and you additionally get live,
in-process interactivity that process-chaining can't.

---

## Where the pieces live

- `src/core/rofi_session.rs` — the persistent child process (spawn, stdin/stdout,
  event sender, EOF/kill).
- `src/core/rofi_protocol.rs` — the parser/formatter for commands, rows, and
  events.
- `src/ui/launcher/state.rs` — `start_rofi_session()` (`:1214`), event
  construction (`:1884`), and the one-shot `ArgumentInput`/`Confirming`
  states.
- `docs/scripts.md` — the complete protocol reference.
- `examples/scripts/two-step.sh`, `theme_switcher.py`, `clipboard.py` —
  working multi-step examples.
