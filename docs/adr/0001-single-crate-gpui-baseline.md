# 1. Single-Crate Layout, GPUI Pinning, and RSS Performance Baseline

* Status: Decided
* Date: 2026-08-31

## Context
aeroFi is designed to be a lightweight keyboard-driven launcher on macOS. To maintain this value proposition, it must prevent memory and CPU footprint bloat typical of web-based or heavy desktop frameworks. At the same time, we use GPUI (a GPU-accelerated UI framework from Zed), which is highly performant but pre-1.0 and experiences frequent breaking API changes. We also want to keep the developer workflow straightforward without path-dependency overhead.

## Decision
1. **Single-Crate Layout**: We organize the codebase in a single crate structured by concern (`ui`, `core`, `sys`, `common`) to avoid multi-crate orchestration and complex dependency loops.
2. **GPUI Pinning**: GPUI is pinned to a specific git commit SHA in `Cargo.toml`. Upgrading GPUI is treated as a separate, isolated task that requires verifying compatibility and performance.
3. **RSS Baseline**: We enforce strict memory baselines:
   - Idle/background RSS must remain under 30 MB (verified after macOS Metal buffer compression).
   - Active/foreground RSS must remain under 40 MB with the search index fully loaded.
   - Hotkey-to-rendered-frame latency must stay under 5 ms.

## Consequences
- **Positive**:
  - Extremely simple build setup (`cargo build`/`cargo run`).
  - Strict performance limits prevent slow accretion of memory overhead (bloat).
  - Pinned GPUI ensures build stability and guards against upstream breaking changes.
- **Negative / Trade-offs**:
  - Upgrading GPUI requires manually updating code for breaking APIs.
  - Development speed may be slightly slowed by needing to justify memory-heavy features or perform FFI audits.
