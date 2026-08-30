# 2. Carbon-Based Global Hotkeys Subsystem

* Status: Proposed
* Date: 2026-08-29

## Context
Users need the ability to execute specific scripts or actions directly via global hotkeys without opening the launcher UI (e.g., spawning a new terminal instance). 

## Decision
We will extend the existing macOS Carbon API (`RegisterEventHotKey`) implementation in `sys/carbon.rs` to support arbitrary `[global_shortcuts]` defined in `config.toml`. Carbon will remain the default hotkey engine.

## Consequences
### Positive
- Zero extra CPU overhead and zero latency during regular typing.
- No Accessibility permissions required for initial setup out-of-the-box.
- Reuses the existing Carbon event loop infrastructure.

### Negative / Trade-offs
- Carbon has blind spots when self-drawing terminal windows (WezTerm/Ghostty/Zed) hold exclusive focus.
- Global executions bypass the in-memory `History`, so they do not feed frecency until a restart re-loads the file.
- No GUI conflict-detection; hand-editing TOML is required, with conflict warnings emitted strictly via `stderr` (settings GUI is an explicit v1 non-goal).
