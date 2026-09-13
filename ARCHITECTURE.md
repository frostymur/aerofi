# ARCHITECTURE.md — aerofi

This document is the source of truth for how aerofi is shaped and why.

## System layers

Single-crate (monorepo style), organized by concern:

```
src/
├── main.rs              # GPUI initialization, hotkey registration, event loop
├── ui/                  # Everything that renders (depends on GPUI)
│   ├── window.rs        # Window setup (borderless PopUp NSPanel, focus & lifecycle)
│   ├── toast_window.rs  # Compact & silent script status notifications (floating toast)
│   ├── execute.rs       # Script execution routing by mode (silent, compact, inline, fullOutput, pipe, gui)
│   └── launcher/        # Input field, search list, keyboard handlers, custom widgets & GUI mode rendering
├── core/                # Data models, config & execution engine (knows nothing about GPUI)
│   ├── item.rs          # Target (App/Script/Builtin), ScriptMode, metadata parsing
│   ├── config.rs        # AppConfig loader (~/.config/aerofi/config.toml) & defaults
│   ├── scanner.rs       # Directory indexing: /Applications* + configured script folders
│   ├── executor.rs      # Launching targets (open, interpreter commands, pbcopy)
│   ├── history.rs       # Launch history & frecency calculation
│   ├── gui_protocol.rs  # Rofi-compatible GUI mode protocol parser
│   ├── gui_session.rs   # GUI mode interactive process session management
│   ├── markdown.rs      # Markdown AST parser for full-output script reader
│   ├── pango.rs         # Pango markup parsing for rich rows
│   ├── search.rs        # Zero-allocation Nucleo fuzzy matcher & frecency ranker
│   ├── theme.rs         # Theme configuration parser, alpha channels & palette resolver
│   └── widget.rs        # Widget tree definition and validation
└── sys/                 # System calls (macOS-only)
    ├── carbon.rs        # Carbon RegisterEventHotKey global hotkey bindings
    ├── appkit.rs        # NSWindow/NSApplication FFI (chrome, transparency, show/hide)
    └── icons.rs         # Native macOS .app icon extraction
```

**Rationale:** single crate keeps the build simple (one `cargo build`, no
path-dependency headaches). The layering (core → ui → sys) enforces separation
of concerns *within* the crate.

## Validated performance baseline

Current operational baseline: **~40 MB RSS idle, ~50 MB RSS active, ~0.1% CPU idle**, fully interactive GPUI window. Treat this as the baseline to protect:

- Idle/backgrounded RSS: keep around ~40 MB (textures dropped and Metal buffers compressed once hidden).
- Active/foreground RSS: keep around ~50 MB with the search index and applications loaded.
- Hotkey-to-rendered-frame latency: ~1–2 ms on Apple Silicon (strict < 5 ms ceiling on the warm path).

Any PR that grows active RSS by more than ~10% needs a one-line justification in the PR description. Measure with Activity Monitor or `footprint <pid>`, before and after hiding the window.

## Hotkey subsystem

Default path: Carbon `RegisterEventHotKey` (via Carbon FFI). This is the only public macOS API for a global hotkey that requires no Accessibility permission — do not require Accessibility just to install the app.

Known limitation, not a bug to "fix" by switching defaults: Carbon `RegisterEventHotKey` silently fails to fire when the frontmost app is a self-drawn text UI — this includes GPU-rendered terminals (WezTerm, Ghostty, Zed's own terminal), which is exactly where this app's users spend most of their time. The fix is a second, **opt-in** backend using `NSEvent.addGlobalMonitorForEvents`, gated behind Accessibility permission and an explicit config flag (`hotkey.reliable_mode = true`). Never make this the default — it trades zero-friction install for reliability, and that trade should be the user's choice, not ours.

## GPUI dependency policy

GPUI is pinned to a specific git commit SHA in `Cargo.toml`, never `main` and never a floating branch. GPUI is pre-1.0 with breaking changes expected between revisions. Bumping the pin is a deliberate PR on its own — not bundled with feature work — that must (a) pass the full test suite and (b) re-verify the RSS baseline above before merging.

## Script execution modes

- `silent`: Runs detached in the background, launcher window closes immediately; floating toast displays status.
- `compact`: Floating toast shows running indicator, outputs single-line status updates.
- `inline`: Displays output dynamically as a subtitle next to the script in the launcher list.
- `fullOutput`: Renders stdout in the built-in markdown viewer with ANSI/Markdown support.
- `pipe`: Captures stdout and copies output directly to the system clipboard (`pbcopy`).
- `gui`: Two-way interactive Rofi-compatible streaming protocol via stdin/stdout (`\0prompt`, `\0message`, etc.).

Applications open via `open <path>`.

## Script metadata: Raycast Script Commands compatible, not extension compatible

The indexer recognizes `# @raycast.title`, `# @raycast.mode`, `# @raycast.icon`, `# @raycast.packageName`, and `# @raycast.argument*` comment tags as first-class, alongside native tags. Existing Raycast script commands work unmodified when dropped into the scripts folder.

We do **not** build a React/TypeScript extension runtime, and we do not attempt live compatibility with the Raycast Store. This is permanent, not a scope cut — it requires chasing a third party's evolving API surface indefinitely and requires bundling a JS runtime, which directly undermines the RSS budget above.

## Explicit non-goals (not "later" — architecturally excluded from v1)

- Windows/Linux support.
- A settings GUI panel — config stays a hand-edited `config.toml` for v1.
- Any heavy JavaScript/React plugin extension runtime (we support native C ABI plugins instead, see below).
- AeroSpace/yabai-native quick actions — genuinely valuable, but not part of the core daemon; when it happens, it should be scripts shelling out to the `aerospace` CLI, not a special-cased integration.

## Native Plugins (C ABI)

Aerofi supports a dynamic plugin system via C ABI (`.dylib`). Plugins are isolated, compiled shared libraries loaded at runtime. This allows developers to extend Aerofi with complex features (file search, window management) using Rust, C, Swift, or Zig without recompiling the core launcher, while preserving the strict zero-overhead performance requirements. The plugin's `activate` function is strictly non-blocking and must execute any heavy workloads in a background thread to maintain the 120 FPS UI loop.

