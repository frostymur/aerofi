# aerofi Roadmap

Ideas and proposals that came out of development discussions. Not a
commitment — items may be reworked, split, or dropped.

## Script UX (Rofi protocol extensions)

- [ ] **Hover / active-item events.** Send a `\0event\x1factive...` line
  to the running script when the selection changes (arrows, mouse), so
  scripts can react to navigation, not just typing and Enter. Currently
  scripts only see `change` / `select` / `action` / `custom`.
- [ ] **Dynamic right-panel preview (Raycast-style Detail View).** Built
  on hover events: the script pushes `\0preview\x1f<markdown>` (or
  `\0preview-file`) as the user moves through the list, so the preview
  panel updates per item. Gives split-view clipboard/file managers a
  full text/image preview of the highlighted entry.
- [ ] **Image rendering in the markdown preview.** `render_md_block`
  currently skips `![alt](path)` tags (see `core/markdown.rs`); render
  them with GPUI `img()` so `\0preview` can show image entries.
- [ ] **Action Menus (`Cmd+K`).** A native action menu over the list:
  built-in entries for apps/builtins (Open, Reveal in Finder, Copy
  Path) plus script-declared actions via an `\0actions\x1f...` row
  attribute, with the chosen action delivered back as
  `\0event\x1faction...`.
- [ ] **Declarative forms.** A `\0form\x1f[...]` protocol command
  (text fields, dropdowns, checkboxes) rendered natively by the
  launcher; answers returned to the script over stdin as structured
  data. Raycast-grade multi-step inputs without an SDK.

## Core / indexer

- [ ] **Filesystem watcher for script folders** (the `notify`-based
  TODO in `core/scanner.rs`): new/removed scripts appear without a
  restart or manual Cmd+R.
