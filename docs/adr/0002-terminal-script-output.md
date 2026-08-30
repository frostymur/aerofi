# 2. Terminal-Based Script Output and Deferred UI Piping

* Status: Decided
* Date: 2026-08-31

## Context
When running shell scripts, they generate output (stdout/stderr) and sometimes require interactive terminal inputs (like `read` prompts or `fzf` selections). Capturing stdout/stderr and displaying it directly inside the GPUI window in real-time requires complex asynchronous process piping, stream parsing, and rapid UI updates.

## Decision
For v0.1, scripts are executed as child processes via `sh <script>` with standard I/O streams inherited directly from the terminal that spawned the aeroFi daemon. Built-in integration for capturing and rendering script output in the UI is deferred to v0.2+.

## Consequences
- **Positive**:
  - Significantly reduces initial UI and process orchestration complexity.
  - Interactive scripts work out-of-the-box (since they run directly in the user's terminal session).
  - No CPU or memory overhead is spent on UI stream piping.
- **Negative / Trade-offs**:
  - Scripts must be run from a terminal to see their output; running the daemon in the background without an attached terminal makes script stdout/stderr invisible.
  - Limits the UI experience to starting actions rather than viewing output.
