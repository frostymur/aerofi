# Contributing to aerofi

## Philosophy

aerofi exists to prove that a launcher can be genuinely light — the
validated baseline (~38 MB active, ~0.2% idle CPU) is the product's whole
pitch against Electron/Qt/WebView alternatives. Any contribution that
regresses that baseline without a clear justification will be rejected
regardless of how useful the feature is. See `ARCHITECTURE.md` for the
full rationale and the current numbers to protect.

## Getting set up

```bash
git clone <repo>
cd aerofi
cargo build
pre-commit install
```

Single crate, simple build: `cargo build` compiles everything, `cargo run`
starts the app (aerofi daemon listens for Alt+Space).

## Branching & workflow

- `main` is always releasable and protected — no direct pushes.
- Trunk-based: branch as `feature/<slug>` or `fix/<slug>` off `main`, open
  a PR, squash-merge.
- Any PR that touches the workspace layout, the IPC protocol, or the
  hotkey backend should be preceded by a short discussion in an issue and
  land with an ADR (see `ARCHITECTURE.md`), not just code.

## Commit messages — Conventional Commits

```
<type>(<scope>): <short, imperative description>
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `ci`
**Scopes:** `ui`, `core`, `sys`, `search`, `indexer`, `config`, `hotkey`

Examples:

```
feat(hotkey): add opt-in NSEvent backend for reliable-mode
fix(indexer): recognize @raycast.argument* tags
perf(daemon): drop idle RSS by lazily loading nucleo index
```

`perf` commits should include the before/after RSS number in the body.

## Versioning & releases

SemVer, tags `vX.Y.Z` on the single crate. Release notes generated from
Conventional Commits since the last tag. Releases are cut from `main` only.

## Testing

- `common/` (metadata parser): unit-test against fixture scripts — at
  minimum one native-format script (`# @name`), one `@raycast.*`-format
  script, one malformed/missing-metadata script (must fail gracefully, not
  panic).
- `core/` (scanner + search): unit-test the indexer (directory scan,
  filtering) and nucleo-matcher integration against fixture script trees.
- `ui/` + `sys/`: hotkey registration is the hardest to test in CI (no
  display, no Carbon support on GitHub Actions runner) — mock the hotkey
  trigger in tests rather than skipping coverage entirely. UI/rendering is
  smoke-tested manually before release, not asserted in CI.
- Every RSS/CPU regression that gets fixed in production becomes a
  regression test or a documented manual-check step, not just a bugfix.

## Code style

`cargo fmt` and `cargo clippy --workspace -- -D warnings`, enforced by
pre-commit and CI. No `unwrap()`/`expect()` outside of tests and `main.rs`
startup code — the daemon runs unattended in the background and should
never crash silently.

## License policy

MIT, applied workspace-wide. No AGPL/GPL/SSPL dependency anywhere in the
tree, including the hotkey and GPUI bindings — check crate licenses before
adding a dependency, not after.

## Contributor sign-off (DCO)

Commit with `git commit -s` so each commit carries a `Signed-off-by:` line
confirming you have the right to submit the change under the project's
license. No CLA.

## Security

aerofi needs no network access for v1 — the daemon only listens on a local
Unix socket. Do not add outbound network calls (telemetry, update checks,
etc.) without a dedicated issue discussion first; "no network access
needed" is a stated design property, not an accident. Report vulnerabilities
privately via `SECURITY.md`, not as a public issue.

## Governance

Early-stage: maintainer-led on architecture decisions. Revisit this once
the project has three or more regular contributors.