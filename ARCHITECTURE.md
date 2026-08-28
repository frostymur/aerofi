# ARCHITECTURE.md — aerofi

This document is the source of truth for how aerofi is shaped and why.
It supersedes `PROJECT_RULES.md`. If a PR conflicts with this document,
the PR is wrong until this document is deliberately amended via an ADR.

## System layers

Single-crate (monorepo style), organized by concern:

```
src/
├── main.rs              # GPUI initialization, hotkey registration, event loop
├── common/              # Shared types (no GPUI, no platform FFI)
│   ├── config.rs        # Config, ThemeColors, WindowConfig
│   ├── script_item.rs   # ScriptItem, ExecutionMode, @raycast.* metadata
│   └── ipc_protocol.rs  # PromptReq, IpcResponse (unused in v0.1, reserved for v0.2)
├── ui/                  # Everything that renders (depends on GPUI)
│   ├── window.rs        # Window setup (borderless, focus management)
│   └── launcher.rs      # Input field, search list, keyboard handlers
├── core/                # Pure business logic (knows nothing about GPUI)
│   ├── item.rs          # ScriptItem parsing, metadata extraction
│   ├── scanner.rs       # Filesystem watcher, directory indexing
│   └── search.rs        # nucleo-matcher wrapper
└── sys/                 # System calls (macOS-only for v0.1, so flat — no per-OS nesting)
    ├── carbon.rs        # Carbon RegisterEventHotKey binding
    └── appkit.rs        # NSWindow/NSApplication FFI (chrome, show/hide)
```

**Rationale:** single crate keeps the build simple (one `cargo build`, no
path-dependency headaches) for v0.1. The layering (common → core → ui →
sys) enforces separation of concerns *within* the crate. If a future
v0.2 adds `aerofi-ask` CLI, the `common/` types are already isolated and
trivial to extract into a separate crate then.

## Validated performance baseline

As of the first working prototype: **~38 MB RSS while active, ~0.2% CPU
idle**, fully interactive GPUI window. Treat this as the baseline to
protect, not a one-time measurement to forget:

- Idle/backgrounded RSS: keep under 20 MB (macOS compresses Metal buffers
  once the window is hidden — verify this after every dependency bump,
  don't assume it holds).
- Active/foreground RSS: keep under 50 MB with the script index loaded.
- Hotkey-to-rendered-frame latency: under 5 ms on the warm path.

Any PR that grows active RSS by more than ~10% needs a one-line
justification in the PR description. Measure with Activity Monitor or
`footprint <pid>`, before and after hiding the window.

## Hotkey subsystem

Default path: Carbon `RegisterEventHotKey` (via the `carbonhotkey` crate or
equivalent). This is the only public macOS API for a global hotkey that
requires no Accessibility permission — do not require Accessibility just to
install the app.

Known limitation, not a bug to "fix" by switching defaults: Carbon
`RegisterEventHotKey` silently fails to fire when the frontmost app is a
self-drawn text UI — this includes GPU-rendered terminals (WezTerm,
Ghostty, Zed's own terminal), which is exactly where this app's users
spend most of their time. The fix is a second, **opt-in** backend using
`NSEvent.addGlobalMonitorForEvents`, gated behind Accessibility permission
and an explicit config flag (`hotkey.reliable_mode = true`). Never make
this the default — it trades zero-friction install for reliability, and
that trade should be the user's choice, not ours.

## GPUI dependency policy

GPUI is pinned to a specific git commit SHA in `Cargo.toml`, never `main`
and never a floating branch. GPUI is pre-1.0 with breaking changes expected
between revisions. Bumping the pin is a deliberate PR on its own — not
bundled with feature work — that must (a) pass the full test suite and
(b) re-verify the RSS baseline above before merging.

## IPC protocol and socket policy

- Socket path: `$TMPDIR/aerofi.sock` or
  `~/Library/Application Support/aerofi/aerofi.sock` — never a hardcoded
  path under shared `/tmp`.
- On startup, the daemon detects a stale socket (file exists, no process
  listening) and removes it before binding. A bind failure on a live socket
  means another daemon instance is already running — exit cleanly with a
  clear message, don't silently steal the socket.
- Protocol: JSON over the socket, request/response, one exchange per
  connection. Types live in `aerofi-core::ipc_protocol`.

## Script execution: terminal-based output in v0.1

Scripts run in a spawned terminal (default: $TERMINAL or WezTerm) via
`open -a <terminal> -- bash -c "<script>"`. The script's stdout/stderr are
visible in the terminal window, which remains open until the user closes it
or runs another command.

**Rationale:** This is simple, honest, and defers UI complexity. If a
script needs interactive output (a menu, a prompt), it can use its own
tooling (`read`, `fzf`, `osascript`). The launcher's job is to find and
run the script, not to build a shell inside the UI.

Deferred for v0.2+: capturing stdout and rendering it in the aerofi UI
(Script Kit style) would require async piping + real-time GPUI updates. This
is worth doing eventually, but not required for v0.1 functionality to be
complete and useful.

## Script metadata: Raycast Script Commands compatible, not extension compatible

The indexer recognizes `# @raycast.title`, `# @raycast.mode`,
`# @raycast.icon`, `# @raycast.packageName`, and `# @raycast.argument*`
comment tags as first-class, alongside the native `@name` / `@description`
/ `@icon` / `@shortcut` / `@mode` tags. Existing Raycast script commands
should work unmodified when dropped into the scripts folder.

We do **not** build a React/TypeScript extension runtime, and we do not
attempt live compatibility with the Raycast Store. This is permanent, not
a v0.1 scope cut — it requires chasing a third party's evolving API
surface indefinitely (this is specifically what makes Vicinae's Raycast
extensions crash over time) and it means bundling a JS runtime, which
directly undermines the RSS budget above.

## Explicit non-goals (not "later" — architecturally excluded from v1)

- Windows/Linux support.
- A settings GUI panel — config stays a hand-edited `config.toml` for v1.
- Any plugin/extension runtime.
- AeroSpace/yabai-native quick actions — genuinely valuable (see roadmap),
  but not part of the core daemon; when it happens, it should be scripts
  shelling out to the `aerospace` CLI, not a special-cased integration.
- UI-based forms or interactive prompts (`aerofi-ask`, JSON-schema forms,
  etc.) — v0.1 scripts that need interactivity use their own tooling
  (`read`, `fzf`, `osascript`) inside the spawned terminal. Capturing
  stdout and rendering it in the aerofi UI is deferred to v0.2.

## ADR process

Any decision expensive to reverse gets a short ADR under `docs/adr/` using
Context / Decision / Status / Consequences. The sections above already
constitute ADR 0001 (single-crate layout + GPUI pin + baseline RSS), ADR
0002 (terminal-based script output, deferred UI piping), and ADR 0003
(Raycast script-command compat, not extension compat) — write those up
formally in `docs/adr/` rather than leaving them only in this file, so
future contributors see the dated record of when and why.