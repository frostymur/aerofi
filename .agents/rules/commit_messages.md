# aerofi — Git Commit Message Rules

## Format

Every commit MUST follow Conventional Commits **exactly**:

```
<type>(<scope>): <short, imperative description>
```

The **type** comes first, the **scope** is in parentheses after the type.
Never swap them (e.g. `ui(theme):` is WRONG — the correct form is `feat(ui):`).

## Allowed types

| Type       | When to use                                      |
|------------|--------------------------------------------------|
| `feat`     | New user-visible feature                         |
| `fix`      | Bug fix                                          |
| `refactor` | Code restructuring with no behaviour change      |
| `perf`     | Performance improvement (must include RSS before/after in body) |
| `test`     | Adding or fixing tests                           |
| `docs`     | Documentation only                               |
| `chore`    | Tooling, deps, CI, formatting                    |
| `ci`       | CI/CD pipeline changes                           |

## Allowed scopes

`ui` · `core` · `sys` · `search` · `indexer` · `config` · `hotkey`

Do NOT invent sub-scopes like `ui(theme)` or nested scopes. The scope is a
single word from the list above.

## Sign-off

Every commit MUST include a DCO sign-off line. Always pass `-s` to `git commit`:

```bash
git commit -s -m "feat(ui): ..."
```

## `perf` commit body

When type is `perf`, the commit body MUST include before/after RSS figures:

```
Before: ~55 MB RSS
After:  ~29 MB RSS
```

## Examples

```
feat(ui): add dark transparent default theme
fix(sys): clip NSWindow contentView layer to rounded corner radius
refactor(core): extract search index into reusable SearchIndex struct
perf(search): eliminate per-keystroke heap allocations in filter_and_rank

Before: ~57 MB RSS active
After:  ~48 MB RSS active
```

## Bad examples (DO NOT use)

```
ui(theme): ...        ← type and scope are swapped
sys(appkit): ...      ← appkit is not an allowed scope, use sys
feat(ui/theme): ...   ← no sub-scopes
update theme          ← no type, no scope
```
