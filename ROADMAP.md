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

- [x] **Filesystem watcher for script folders** (`notify`-based FSEvents
  watcher, `core/script_watcher.rs`): new/removed scripts appear without
  a restart or manual Cmd+R; `reconcile_daemons` keeps inline daemons in
  sync.

## Modes & navigation

- [ ] **Global hotkey access to plugins.** Plugins are currently only
  reached by typing their prefix. Allow `[bindings.global]` to name a
  plugin directly (show the launcher with the prefix pre-filled), so
  e.g. a file search is one hotkey — rofi-style mode hotkeys without
  typing.
- [ ] **Runtime source filter (Apps / Scripts / All).** A binding or
  builtin action that filters the target list at runtime for
  script-heavy setups, instead of hand-editing `[sources]` in the
  config file (which some setups keep read-only, e.g. Nix-managed).
- [ ] **Mode switcher (rofi-style tabs).** The larger follow-up built
  on the two items above: a `mode-switcher` widget with tab buttons,
  a `Button` selected/active state to show the current mode, and
  named modes combining a source filter, a plugin, or a script
  session.

## Widgets & theming

- [ ] **Scrollbar / scroll position indicator.** Lists scroll without
  a visible indicator (GPUI hides scrollbars, Zed-style). Add a
  themable scrollbar (or at least a thin position bar) to
  `[listview]` so long result sets show where the user is.
- [ ] **Dynamic text widgets.** `text` widgets are static. Add
  built-in dynamic values (e.g. a clock/date format) and a way to
  pipe a script's output into a widget, turning footers into live
  status bars (clock, system stats) without a second tool.
- [ ] **Conditional widget visibility.** A `show_if`-style attribute
  (e.g. query non-empty, Rofi mode, specific script) so buttons and
  badges appear with context instead of always occupying layout
  space.
- [ ] **Gradients in color fields.** Colors currently accept flat
  hex/rgb only. Support gradient values (stops + angle) for
  backgrounds and borders.
