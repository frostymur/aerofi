# Contributing to aerofi

## Philosophy

aerofi exists to prove that a launcher can be genuinely light — the
validated baseline (~50 MB active, ~0.1% idle CPU) is the product's whole
pitch against Electron/Qt/WebView alternatives. Any contribution that
regresses that baseline without a clear justification will be rejected
regardless of how useful the feature is. See `ARCHITECTURE.md` for the
full rationale and the current numbers to protect.

## Getting set up

```bash
git clone <repo>
cd aerofi
cargo test --all-targets
```

All 100+ tests should pass on macOS. There are no platform-independent
stubs — macOS is the only supported target.

## Architectural discipline

- Every new dependency requires justification. Never pull in a crate that
  brings its own runtime (tokio, smol, etc.) — `gpui` has its own async
  executor.
- No second-process architectures. Everything runs in the single `aerofi`
  binary.
- Any change that touches windowing, the metal rendering pipeline, or the
  hotkey backend should be preceded by a short discussion in an issue and
  land with architectural documentation in `ARCHITECTURE.md`, not just code.

## Commit messages — Conventional Commits

```
<type>(<scope>): <short, imperative description>
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `ci`
**Scopes:** `ui`, `core`, `sys`, `search`, `indexer`, `config`, `hotkey`, `theme`, `widgets`, `scripts`, `plugin`

Examples:

```
feat(hotkey): add opt-in NSEvent backend for reliable-mode
fix(indexer): recognize @raycast.argument* tags
perf(search): drop idle RSS by lazily loading nucleo index
```

`perf` commits should include the before/after RSS number in the body.

## Versioning & releases

We adhere to [Semantic Versioning (SemVer)](https://semver.org/) via git tags `vX.Y.Z` on the single crate:

- **PATCH (`z` in `X.Y.Z`, e.g., `0.0.7` -> `0.0.8`)**:
  - Incremented for backward-compatible bug fixes and internal improvements.
  - Triggers: `fix:`, `perf:`, internal refactorings, dependency updates, and maintenance without user-facing breaking changes.
- **MINOR (`y` in `X.Y.Z`, e.g., `0.0.8` -> `0.1.0`)**:
  - Incremented when new backward-compatible functionality is added.
  - Triggers: `feat:` (e.g., new script modes, new theme widgets, new configuration options, new UI capabilities).
- **MAJOR (`x` in `X.Y.Z`, e.g., `0.1.0` -> `1.0.0`)**:
  - Incremented for incompatible breaking changes that require users to alter their `config.toml`, themes, or script protocol usage.
  - Triggers: commits with `BREAKING CHANGE:` in footer or `!` after type (e.g., `feat!:`, `fix!:`).
  - *Pre-1.0 note*: In the `0.Y.Z` phase, breaking changes may bump `Y` while the public API stabilizes toward `1.0.0`.

Release notes are generated from Conventional Commits since the last tag. Releases are cut from `main` only.

We use [`cargo-release`](https://github.com/crate-ci/cargo-release) to automate version bumping, tagging, and pushing:

```bash
# To install cargo-release:
cargo install cargo-release

# To bump version, commit, tag, and push (e.g. for a patch release):
cargo release patch --execute
```

This will automatically bump `Cargo.toml`, create the commit `chore(core): bump version to <version>`, create the tag `v<version>`, and push them. The GitHub Actions release pipeline handles the rest.

## Testing

- `core/` (metadata parser, `item.rs`): unit-test against fixture scripts —
  at minimum one native-format script (`# @name`), one `@raycast.*`-format
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

`cargo fmt` and `cargo clippy --workspace -- -D warnings`, enforced by CI.
No `unwrap()`/`expect()` outside of tests and `main.rs`
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

aerofi needs no network access for v1 — the background scheduler runs as
in-process threads (no daemons, sockets, or sidecar processes). Do not add
outbound network calls (telemetry, update checks, etc.) without a dedicated
issue discussion first; "no network access needed" is a stated design
property, not an accident. Report vulnerabilities privately via
`SECURITY.md`, not as a public issue.

## Governance

Early-stage: maintainer-led on architecture decisions. Revisit this once
the project has three or more regular contributors.

