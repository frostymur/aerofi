//! Window-height measurement for the search UI (the `auto_height` /
//! `require_input` fit pass).
//!
//! **Two-sides rule:** this measurer must mirror the actual layout the
//! renderer walks (`ui/launcher/custom_widgets.rs::render_widget_def`):
//! a vertical box stacks its children plus gaps, a horizontal box takes
//! the tallest child, a fixed `height` is authoritative, a `flex` box
//! measures as its content. Changing a layout rule on either side
//! requires updating this file (and its tests) in the same commit —
//! otherwise `auto_height` windows are mis-sized (too short clips the
//! bar, too tall leaves dead space).
//!
//! The module is deliberately pure (no GPUI, no I/O): it takes the
//! theme, the widget registry, state-dependent inputs
//! ([`SearchFit`]) and pre-resolved image dimensions, and returns a
//! height in points — so the sizing math is unit-testable without a
//! window.
//!
//! Lives in `core` (not `ui`) on purpose: it depends only on the theme
//! and widget model, and keeping it out of the render module makes the
//! renderer/measurer boundary explicit for review.

use crate::core::theme::{BuiltinWidget, ThemeConfig, Widget, WidgetDef};
use std::collections::HashMap;

use crate::core::widget::WidgetRegistry;

/// Returns true if `s` looks like a local image file path. Shared by the
/// measurer (this module) and the renderer (which re-exports it from
/// `ui::launcher::helpers`).
pub fn is_image_path(s: &str) -> bool {
    s.starts_with('/')
        || s.starts_with("./")
        || s.ends_with(".png")
        || s.ends_with(".jpg")
        || s.ends_with(".jpeg")
        || s.ends_with(".webp")
        || s.ends_with(".tiff")
}

/// State-dependent inputs for [`search_fit_height`]. The resolved booleans
/// (not the raw theme fields) are passed in so this stays a pure function of
/// "what is on screen right now".
pub struct SearchFit {
    /// Whether the input bar is rendered. A script's `show_search false`
    /// metatag removes it — together with the mainbox gap that followed it.
    pub show_search: bool,
    /// Whether the results list is rendered this frame.
    pub should_show_list: bool,
    /// Resolved auto-height: fit the window to the results list.
    pub auto_height: bool,
    /// Resolved `listview.require_input`.
    pub require_input: bool,
    /// Input bar height (already wrapped-chip-adjusted for `ArgumentInput`).
    pub ib_h: f32,
    /// Effective list columns (1 = list mode, >1 = grid mode).
    pub columns: usize,
    /// Number of visible items (`filtered.len()`).
    pub filtered_len: usize,
    /// Element font size (`element_font_val.1`), for text-line metrics.
    pub element_font_size: f32,
    /// Screen height the window `Dimension` resolves against.
    pub screen_h: f32,
    /// Screen width the window `Dimension` resolves against (needed to
    /// measure `w_full` images, whose rendered width is the container's).
    pub screen_w: f32,
}

/// Window height for the `Search` / `ArgumentInput` / `Confirming` states.
///
/// Pure function of the theme, widget registry, current state and
/// pre-resolved image dimensions (no GPUI, no I/O) so the sizing math can
/// be unit-tested.
///
/// Instead of assuming a fixed `InputBar + ListView` stack, it measures the
/// same widget tree the renderer walks: a vertical box stacks its rendered
/// children plus gaps, a horizontal box takes the tallest child, and
/// fixed-size widgets (artwork, footers, buttons) contribute their own
/// size — so `auto_height` works with custom `[widgets]` layouts. The
/// scrollable list is capped by the configured window height (long lists
/// scroll), while the fixed chrome sets a floor the window never shrinks
/// below, so artwork and footer bars are never clipped.
///
/// With `require_input` the window collapses to the chrome while the list
/// is hidden (before the first matching keystroke), regardless of
/// `auto_height`; once the list appears, `auto_height` fits it (capped at
/// the configured height) and without it the configured height is used.
///
/// The list/grid heights mirror the exact row and cell heights
/// `render_row` / `render_grid_cell` lay out:
/// - list row: `2·pad + max(icon, text_h) + 2·border`
/// - grid cell: `2·pad + icon + icon_gap + text_h + 2·border`, packed into
///   `len.div_ceil(columns)` rows
pub fn search_fit_height(
    t: &ThemeConfig,
    reg: &WidgetRegistry,
    state: &SearchFit,
    image_dims: &HashMap<String, (u32, u32)>,
) -> f32 {
    let window_h = t.window.height.resolve(state.screen_h);
    if !(state.require_input || state.auto_height) {
        return window_h;
    }

    let pad2 = t.window.padding * 2.0;
    // Fixed chrome (bar, header/footer, artwork) — the floor the window
    // must never shrink below.
    let chrome = mainbox_height(t, reg, state, image_dims, None);
    if !state.should_show_list {
        // `require_input` idle (list hidden): the window collapses to the
        // chrome, independent of `auto_height` — no height and no gaps for
        // the absent list slot.
        return chrome + pad2;
    }
    let list_h = list_height(t, state);
    if state.auto_height {
        let total = mainbox_height(t, reg, state, image_dims, Some(list_h));
        (chrome + pad2).max((total + pad2).min(window_h))
    } else {
        // `require_input` without auto-height: the configured height, but
        // never below the chrome (which wrapped argument chips can make
        // taller than nominal).
        window_h.max(chrome + pad2)
    }
}

/// Height of the results list/grid for `state.filtered_len` items — the
/// same row and cell metrics `render_row` / `render_grid_cell` lay out.
/// Dropping the text-line/border terms under-sizes the window and clips
/// the last row; counting items instead of grid rows over-sizes it in
/// grid themes.
pub fn list_height(t: &ThemeConfig, state: &SearchFit) -> f32 {
    let spacing = t.listview.spacing;
    let el = &t.element;
    let el_pad_v = el.padding.first().copied().unwrap_or(8.0);
    // Same text-line metrics as `render_row` / `render_grid_cell`.
    let list_text_h = state.element_font_size * 1.2;
    let grid_text_h = (state.element_font_size - 1.5).max(11.0) * 1.2;

    if state.columns > 1 {
        let rows = state.filtered_len.div_ceil(state.columns);
        let cell_h =
            el_pad_v * 2.0 + el.icon_size + el.icon_gap + grid_text_h + el.border_width * 2.0;
        rows as f32 * cell_h + (rows.saturating_sub(1)) as f32 * spacing
    } else {
        let row_h = el_pad_v * 2.0 + el.icon_size.max(list_text_h) + el.border_width * 2.0;
        state.filtered_len as f32 * row_h + (state.filtered_len.saturating_sub(1)) as f32 * spacing
    }
}

/// Content height of `[mainbox]` (window padding excluded). `list_slot` is
/// `Some(h)` while the ListView renders at height `h`, `None` when it is
/// absent from the layout (hidden by `require_input`).
fn mainbox_height(
    t: &ThemeConfig,
    reg: &WidgetRegistry,
    state: &SearchFit,
    image_dims: &HashMap<String, (u32, u32)>,
    list_slot: Option<f32>,
) -> f32 {
    let horizontal = t.mainbox.orientation != "vertical";
    let gap = t.mainbox.gap.unwrap_or(t.listview.spacing);
    let mut sum = 0.0f32;
    let mut max_h = 0.0f32;
    let mut rendered = 0usize;
    for child in &t.mainbox.children {
        let h = child_height(t, reg, state, image_dims, list_slot, child);
        if let Some(h) = h {
            rendered += 1;
            sum += h;
            max_h = max_h.max(h);
        }
    }
    // A horizontal container is as tall as its tallest child; the gaps
    // between its children run along the horizontal axis and don't add
    // height.
    if horizontal {
        max_h
    } else {
        sum + gap * (rendered.saturating_sub(1)) as f32
    }
}
/// Height of one `[mainbox]` child slot (built-in or custom widget);
/// `None` means the slot is not rendered this frame. Shared by
/// [`mainbox_height`] and the fit-invariant tests, so "what each child
/// needs" is computed in exactly one place.
fn child_height(
    t: &ThemeConfig,
    reg: &WidgetRegistry,
    state: &SearchFit,
    image_dims: &HashMap<String, (u32, u32)>,
    list_slot: Option<f32>,
    child: &Widget,
) -> Option<f32> {
    match child {
        Widget::Builtin(BuiltinWidget::InputBar) => {
            state.show_search.then_some(inputbar_height(t, state))
        }
        Widget::Builtin(BuiltinWidget::ListView) => list_slot,
        Widget::Custom(id) => widget_height(t, reg, state, image_dims, list_slot, id),
    }
}

/// Height of one child slot: a built-in name (matched case-insensitively,
/// like `render_widget_box`) or a custom widget id. `None` means the slot
/// is not rendered.
fn widget_height(
    t: &ThemeConfig,
    reg: &WidgetRegistry,
    state: &SearchFit,
    image_dims: &HashMap<String, (u32, u32)>,
    list_slot: Option<f32>,
    id: &str,
) -> Option<f32> {
    if id.eq_ignore_ascii_case("inputbar") {
        return state.show_search.then_some(inputbar_height(t, state));
    }
    if id.eq_ignore_ascii_case("listview") {
        return list_slot;
    }
    reg.get(id)
        .and_then(|def| def_height(t, reg, state, image_dims, list_slot, def))
}

/// Intrinsic height of a custom widget definition, mirroring
/// `render_widget_def`'s layout rules: a box stacks its children or takes
/// the tallest one; a fixed `height` is authoritative; a `flex` box
/// measures as its content ("fill the remainder" resolves to the content
/// size in an auto-height window).
fn def_height(
    t: &ThemeConfig,
    reg: &WidgetRegistry,
    state: &SearchFit,
    image_dims: &HashMap<String, (u32, u32)>,
    list_slot: Option<f32>,
    def: &WidgetDef,
) -> Option<f32> {
    match def {
        WidgetDef::Box {
            orientation,
            gap,
            padding,
            margin,
            height,
            border_width,
            border_top,
            border_bottom,
            children,
            ..
        } => {
            if let Some(h) = height {
                return Some(h + v_extent(margin.as_deref()));
            }
            let horizontal = orientation.as_deref().unwrap_or("horizontal") != "vertical";
            let mut sum = 0.0f32;
            let mut max_h = 0.0f32;
            let mut rendered = 0usize;
            for child in children {
                if let Some(h) = widget_height(t, reg, state, image_dims, list_slot, child) {
                    rendered += 1;
                    sum += h;
                    max_h = max_h.max(h);
                }
            }
            let g = gap.unwrap_or(0.0);
            let content = if horizontal {
                max_h
            } else {
                sum + g * (rendered.saturating_sub(1)) as f32
            };
            Some(
                content
                    + v_extent(padding.as_deref())
                    + v_border(*border_width, *border_top, *border_bottom)
                    + v_extent(margin.as_deref()),
            )
        }
        WidgetDef::Image {
            path,
            width,
            height,
            h_full,
            w_full,
            margin,
            border_width,
            border_top,
            border_bottom,
            ..
        } => {
            let h = if h_full.unwrap_or(false) {
                // Stretches to the container: no intrinsic height.
                0.0
            } else if let Some(h) = height {
                *h
            } else if let Some(w) = width {
                // Scaled to the fixed width by the image's aspect ratio
                // (falls back to a square when dimensions are unknown).
                match image_dims.get(path) {
                    Some((nw, nh)) if *nw > 0 => *w * (*nh as f32) / (*nw as f32),
                    _ => *w,
                }
            } else if w_full.unwrap_or(false) {
                // Width-stretched: the renderer scales the height by the
                // aspect ratio to the container width (GPUI `img` keeps
                // its intrinsic aspect ratio when only width is fixed).
                // The container is at most the window wide, so using the
                // window width is the never-clip bound: exact for
                // full-width containers, a bounded overestimate for
                // images inside narrower panes (exact pane widths need a
                // layout pass, out of scope for a height measurer).
                match image_dims.get(path) {
                    Some((nw, nh)) if *nw > 0 => {
                        let w = t.window.width.resolve(state.screen_w);
                        w * (*nh as f32) / (*nw as f32)
                    }
                    _ => 0.0,
                }
            } else {
                image_dims
                    .get(path)
                    .map(|(_, nh)| *nh as f32)
                    .unwrap_or(0.0)
            };
            Some(
                h + v_border(*border_width, *border_top, *border_bottom)
                    + v_extent(margin.as_deref()),
            )
        }
        WidgetDef::Text {
            font_size, margin, ..
        } => Some(font_size.unwrap_or(t.font.size) * 1.2 + v_extent(margin.as_deref())),
        WidgetDef::Icon {
            icon, size, margin, ..
        } => {
            let sz = size.unwrap_or(t.element.icon_size);
            // Image icons render at exactly `size`; glyph icons as a
            // centred line of text (~1.2em).
            let h = if is_image_path(icon) { sz } else { sz * 1.2 };
            Some(h + v_extent(margin.as_deref()))
        }
        WidgetDef::Spacer { .. } => Some(0.0),
        WidgetDef::Divider {
            thickness, margin, ..
        } => {
            // `.h(thickness).my(margin)`.
            Some(thickness.unwrap_or(1.0) + margin.unwrap_or(4.0) * 2.0)
        }
        WidgetDef::Button {
            font_size,
            padding,
            margin,
            ..
        } => {
            // The text line drives the content height; the renderer pins
            // this exactly with an explicit `.h()` (`render_widget_button`)
            // because a natural text line follows the font's own line
            // height (JetBrainsMono ≈ 1.64em) and would desync the
            // measurement — the window would be too short and the list's
            // last row clipped. Default vertical padding: 4pt per side.
            let sz = font_size.unwrap_or(t.font.size);
            let pad = match padding.as_deref() {
                Some(p) => v_extent(Some(p)),
                None => 8.0,
            };
            Some(sz * 1.2 + pad + v_extent(margin.as_deref()))
        }
    }
}

/// Vertical footprint of the input bar: its height plus its bottom margin
/// (the renderer applies only the bottom margin — see `render_inputbar`).
fn inputbar_height(t: &ThemeConfig, state: &SearchFit) -> f32 {
    state.ib_h + t.inputbar.margin.get(2).copied().unwrap_or(8.0)
}
/// Scrollbar thumb geometry for an overflowing scroll view.
///
/// `content_h` is the full scrollable content height, `viewport_h` the
/// visible height, `offset` the current scroll position in content
/// units. Returns `None` when the content fits (no overflow), else the
/// thumb's top offset and height within the viewport. The thumb keeps a
/// minimum size so short overflows stay draggable.
///
/// Pure (no GPUI) so the render side can only *draw* the result — the
/// same "core computes, UI renders" split as [`search_fit_height`].
pub fn scrollbar_metrics(content_h: f32, viewport_h: f32, offset: f32) -> Option<(f32, f32)> {
    if content_h <= viewport_h + 1e-6 || viewport_h <= 0.0 {
        return None;
    }
    const MIN_THUMB: f32 = 24.0;
    let scrollable = content_h - viewport_h;
    let mut thumb_h = (viewport_h * viewport_h / content_h).max(MIN_THUMB);
    if thumb_h > viewport_h {
        thumb_h = viewport_h;
    }
    let thumb_y = if scrollable <= 1e-6 {
        0.0
    } else {
        (offset / scrollable) * (viewport_h - thumb_h)
    };
    Some((thumb_y, thumb_h))
}

/// Vertical extent of a padding/margin array, mirroring the
/// `apply_padding!` / `apply_margin!` macros: one value = all sides, two =
/// `[vertical, horizontal]`, four = `[top, right, bottom, left]`.
fn v_extent(values: Option<&[f32]>) -> f32 {
    match values {
        Some(v) => match v.len() {
            1 | 2 => v[0] * 2.0,
            4 => v[0] + v[2],
            _ => 0.0,
        },
        None => 0.0,
    }
}

/// Top + bottom border widths: `border_width` sets all four sides and the
/// per-side overrides take precedence (mirrors `apply_border`).
fn v_border(width: Option<f32>, top: Option<f32>, bottom: Option<f32>) -> f32 {
    let base = width.unwrap_or(0.0);
    top.unwrap_or(base) + bottom.unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{SearchFit, child_height, mainbox_height, scrollbar_metrics, search_fit_height};
    use crate::core::theme::{BuiltinWidget, Dimension, ThemeConfig, Widget, WidgetDef};
    use crate::core::widget::WidgetRegistry;

    /// Measure with an empty widget registry and no image dimensions (the
    /// default mainbox only references built-ins).
    fn measure(t: &ThemeConfig, state: &SearchFit) -> f32 {
        let reg = WidgetRegistry::from_theme(&t.widgets);
        search_fit_height(t, &reg, state, &HashMap::new())
    }

    fn box_def(
        id: &str,
        orientation: &str,
        gap: Option<f32>,
        padding: Option<&[f32]>,
        children: Vec<&str>,
    ) -> WidgetDef {
        WidgetDef::Box {
            id: id.to_string(),
            orientation: Some(orientation.to_string()),
            gap,
            padding: padding.map(<[f32]>::to_vec),
            margin: None,
            align: None,
            background: None,
            radius: None,
            border_color: None,
            border_width: None,
            border_top: None,
            border_right: None,
            border_bottom: None,
            border_left: None,
            shadow: None,
            width: None,
            height: None,
            flex: None,
            children: children.into_iter().map(String::from).collect(),
        }
    }

    fn button_def(id: &str, font_size: f32, padding: Option<&[f32]>) -> WidgetDef {
        WidgetDef::Button {
            id: id.to_string(),
            text: Some(id.to_string()),
            icon: None,
            action: Some("noop".to_string()),
            hotkey: None,
            color: None,
            background: None,
            hover_background: None,
            hover_color: None,
            border_color: None,
            border_width: None,
            border_top: None,
            border_right: None,
            border_bottom: None,
            border_left: None,
            shadow: None,
            radius: None,
            padding: padding.map(<[f32]>::to_vec),
            margin: None,
            font_size: Some(font_size),
            font_weight: None,
            gap: None,
            close: None,
        }
    }

    fn text_def(id: &str, font_size: f32) -> WidgetDef {
        WidgetDef::Text {
            id: id.to_string(),
            text: Some(id.to_string()),
            color: None,
            font_size: Some(font_size),
            font_weight: None,
            align: None,
            margin: None,
        }
    }

    fn icon_def(id: &str, icon: &str, size: f32) -> WidgetDef {
        WidgetDef::Icon {
            id: id.to_string(),
            icon: icon.to_string(),
            size: Some(size),
            color: None,
            margin: None,
        }
    }

    fn spacer_def(id: &str) -> WidgetDef {
        WidgetDef::Spacer { id: id.to_string() }
    }

    fn image_def_full(id: &str, path: &str) -> WidgetDef {
        let mut def = image_def(id, path, None, None);
        if let WidgetDef::Image { w_full, .. } = &mut def {
            *w_full = Some(true);
        }
        def
    }

    fn image_def(id: &str, path: &str, width: Option<f32>, height: Option<f32>) -> WidgetDef {
        WidgetDef::Image {
            id: id.to_string(),
            path: path.to_string(),
            width,
            height,
            radius: None,
            w_full: None,
            h_full: None,
            margin: None,
            border_color: None,
            border_width: None,
            border_top: None,
            border_right: None,
            border_bottom: None,
            border_left: None,
            shadow: None,
        }
    }

    /// Deterministic theme: fixed 500pt window so `screen_h` is irrelevant.
    fn fit_theme() -> ThemeConfig {
        let mut t = ThemeConfig::default();
        t.window.height = Dimension::Points(500.0);
        t.window.padding = 10.0;
        t.inputbar.height = 40.0;
        t.inputbar.margin = vec![0.0, 0.0, 8.0, 0.0]; // bottom = 8
        t.mainbox.gap = Some(12.0);
        t.listview.spacing = 6.0;
        t.element.padding = vec![6.0, 10.0];
        t.element.icon_size = 24.0;
        t.element.icon_gap = 4.0;
        t.element.border_width = 0.0;
        t
    }

    fn fit(
        show_search: bool,
        show_list: bool,
        auto_height: bool,
        require_input: bool,
        columns: usize,
        n: usize,
        font: f32,
    ) -> SearchFit {
        SearchFit {
            show_search,
            should_show_list: show_list,
            auto_height,
            require_input,
            ib_h: 40.0,
            columns,
            filtered_len: n,
            element_font_size: font,
            screen_h: 1000.0,
            screen_w: 1400.0,
        }
    }

    /// List mode, font (20.4) shorter than icon (24): row = 2·6 + 24 + 0 = 36.
    #[test]
    fn list_mode_fits_rows_exactly() {
        let t = fit_theme();
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        // bar(40) + margin(8) + gap(12) + [3·36 + 2·6] + pad(20) = 200
        assert!((h - 200.0).abs() < 1e-3, "got {h}");
    }

    /// Fix A: element borders were omitted from the row height, under-sizing
    /// the window and clipping the last row.
    #[test]
    fn list_mode_counts_element_borders() {
        let mut t = fit_theme();
        t.element.border_width = 1.0;
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        // row = 2·6 + 24 + 2·1 = 38 → 40+8+12 + [3·38 + 2·6] + 20 = 206
        assert!((h - 206.0).abs() < 1e-3, "got {h}");
    }

    /// Fix A: a font taller than the icon drives the row height.
    #[test]
    fn list_mode_uses_taller_text_line() {
        let t = fit_theme();
        // font 30 → text_h 36 > icon 24 → row = 12 + 36 = 48
        let h = measure(&t, &fit(true, true, true, true, 1, 2, 30.0));
        // 40+8+12 + [2·48 + 6] + 20 = 182
        assert!((h - 182.0).abs() < 1e-3, "got {h}");
    }

    /// Fix B: grid themes pack `n.div_ceil(columns)` rows, not `n`.
    #[test]
    fn grid_mode_counts_rows_not_items() {
        let t = fit_theme();
        // cell = 2·6 + 24 + 4 + (17-1.5)·1.2 + 0 = 58.6; 8 items / 4 cols = 2 rows
        let h = measure(&t, &fit(true, true, true, true, 4, 8, 17.0));
        // 40+8+12 + [2·58.6 + 6] + 20 = 203.2
        assert!((h - 203.2).abs() < 1e-3, "got {h}");
    }

    /// Fix B: a partially-filled last row still occupies a full row.
    #[test]
    fn grid_mode_partial_last_row() {
        let t = fit_theme();
        // 10 items / 4 cols = 3 rows
        let h = measure(&t, &fit(true, true, true, true, 4, 10, 17.0));
        // 40+8+12 + [3·58.6 + 2·6] + 20 = 267.8
        assert!((h - 267.8).abs() < 1e-3, "got {h}");
    }

    /// Fix C: `show_search false` removes the bar, its margin and the gap.
    #[test]
    fn hidden_bar_drops_bar_and_gap() {
        let t = fit_theme();
        let with_bar = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        let without_bar = measure(&t, &fit(false, true, true, true, 1, 3, 17.0));
        // bar(40) + margin(8) + gap(12) = 60 less
        assert!((with_bar - without_bar - 60.0).abs() < 1e-3);
        assert!((without_bar - 140.0).abs() < 1e-3, "got {without_bar}");
    }

    /// Compact (list hidden) with the bar: the bar's full vertical
    /// footprint (height + bottom margin) plus the window padding.
    #[test]
    fn compact_shows_bar_only() {
        let t = fit_theme();
        let h = measure(&t, &fit(true, false, true, true, 1, 0, 17.0));
        // bar(40) + margin(8) + pad(20) = 68
        assert!((h - 68.0).abs() < 1e-3, "got {h}");
    }

    /// Compact with the bar hidden too: only the window padding remains.
    #[test]
    fn compact_without_bar_is_padding_only() {
        let t = fit_theme();
        let h = measure(&t, &fit(false, false, true, true, 1, 0, 17.0));
        assert!((h - 20.0).abs() < 1e-3, "got {h}");
    }

    /// Long lists cap at the configured window height.
    #[test]
    fn fits_are_capped_at_window_height() {
        let t = fit_theme();
        let h = measure(&t, &fit(true, true, true, true, 1, 1000, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// require_input without auto-height: full height, never below the bar.
    #[test]
    fn require_input_without_auto_height_is_full() {
        let t = fit_theme();
        let h = measure(&t, &fit(true, true, false, true, 1, 3, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// Regression: require_input without auto-height still collapses the
    /// window to the chrome while the list is hidden (idle) — auto_height
    /// only changes what happens once the list appears.
    #[test]
    fn require_input_without_auto_height_collapses_idle() {
        let t = fit_theme();
        // bar(40) + margin(8) + pad(20) = 68 — not the 500pt window height.
        let h = measure(&t, &fit(true, false, false, true, 1, 0, 17.0));
        assert!((h - 68.0).abs() < 1e-3, "got {h}");
    }

    /// Same collapse in a custom layout: the chrome includes the footer bar.
    #[test]
    fn require_input_without_auto_height_collapses_to_chrome() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def(
                "footer",
                "horizontal",
                Some(8.0),
                Some(&[8.0, 4.0]),
                vec!["hint", "flex", "btn_a", "btn_b"],
            ),
            text_def("hint", 11.0),
            spacer_def("flex"),
            button_def("btn_a", 11.0, Some(&[4.0, 8.0])),
            button_def("btn_b", 11.0, Some(&[4.0, 8.0])),
        ];
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("footer".into()),
        ];
        // chrome = bar(48) + gap(12) + footer(37.2) = 97.2 → + pad 20
        let h = measure(&t, &fit(true, false, false, true, 1, 0, 17.0));
        assert!((h - 117.2).abs() < 1e-3, "got {h}");
        // ...and typing expands to the full configured height.
        let h = measure(&t, &fit(true, true, false, true, 1, 3, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// Neither flag: plain full height, no fitting at all.
    #[test]
    fn no_fitting_is_full_height() {
        let t = fit_theme();
        let h = measure(&t, &fit(true, true, false, false, 1, 3, 17.0));
        assert!((h - 500.0).abs() < 1e-3, "got {h}");
    }

    /// Decoupled case: auto-height on without require-input (list always shown).
    #[test]
    fn auto_height_without_require_input_fits() {
        let t = fit_theme();
        let h = measure(&t, &fit(true, true, true, false, 1, 3, 17.0));
        assert!((h - 200.0).abs() < 1e-3, "got {h}");
    }

    /// A horizontal footer bar (text + spacer + two buttons) below the list
    /// adds its own height plus one mainbox gap to the standard fit.
    #[test]
    fn footer_bar_is_counted() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def(
                "footer",
                "horizontal",
                Some(8.0),
                Some(&[8.0, 4.0]),
                vec!["hint", "flex", "btn_a", "btn_b"],
            ),
            text_def("hint", 11.0),
            spacer_def("flex"),
            button_def("btn_a", 11.0, Some(&[4.0, 8.0])),
            button_def("btn_b", 11.0, Some(&[4.0, 8.0])),
        ];
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("footer".into()),
        ];
        // footer = max(text 13.2, spacer 0, buttons 13.2 + 8) + pad 16 = 37.2
        // 48 + 12 + 120 + 12 + 37.2 = 229.2 → + pad 20 = 249.2
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        assert!((h - 249.2).abs() < 1e-3, "got {h}");
    }
    /// Regression: a button bar must measure exactly `sz * 1.2 + v-pad`
    /// per button. The renderer pins this with an explicit height
    /// (`render_widget_button`); before the pin, fonts with a tall line
    /// height (JetBrainsMono ≈ 1.64em) made the rendered bar taller than
    /// the measurement, and the auto_height window clipped the list's
    /// last row.
    #[test]
    fn button_bar_height_is_exact_contract() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def("bar", "horizontal", Some(6.0), None, vec!["a", "b", "c"]),
            button_def("a", 11.0, Some(&[3.0, 8.0])),
            button_def("b", 11.0, Some(&[3.0, 8.0])),
            button_def("c", 11.0, Some(&[3.0, 8.0])),
        ];
        t.mainbox.children = vec![
            Widget::Custom("bar".into()),
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
        ];
        // bar = max(button 11*1.2 + 3*2 = 19.2) = 19.2; chrome = 19.2 +
        // gap 12 + bar 40 + margin 8 = 79.2; + pad 20 → idle 99.2.
        let h = measure(&t, &fit(true, false, false, true, 1, 0, 17.0));
        assert!((h - 99.2).abs() < 1e-3, "got {h}");
        // With the list (1 row: pad 6*2 + max(icon 24, 20.4) = 36):
        // 79.2 + 12 + 36 = 127.2 → + pad 20 = 147.2.
        let h = measure(&t, &fit(true, true, true, true, 1, 1, 17.0));
        assert!((h - 147.2).abs() < 1e-3, "got {h}");
    }

    /// A header bar (icon + title + spacer) above the input bar is counted
    /// the same way.
    #[test]
    fn header_bar_is_counted() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def(
                "header",
                "horizontal",
                Some(8.0),
                Some(&[4.0, 4.0]),
                vec!["logo", "title", "flex"],
            ),
            icon_def("logo", "⚡", 14.0),
            text_def("title", 12.0),
            spacer_def("flex"),
        ];
        t.mainbox.children = vec![
            Widget::Custom("header".into()),
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
        ];
        // header = max(glyph 16.8, text 14.4, spacer 0) + pad 8 = 24.8
        // 24.8 + 12 + 48 + 12 + 120 = 216.8 → + pad 20 = 236.8
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        assert!((h - 236.8).abs() < 1e-3, "got {h}");
    }

    /// Horizontal split with a fixed 480pt artwork: the window never shrinks
    /// below the artwork (it even grows past the configured 300pt height),
    /// and long lists scroll instead of stretching the window.
    #[test]
    fn split_layout_artwork_is_never_clipped() {
        let mut t = fit_theme();
        t.window.height = Dimension::Points(300.0);
        t.window.padding = 0.0;
        t.mainbox.orientation = "horizontal".into();
        t.mainbox.gap = Some(0.0);
        t.mainbox.children = vec![
            Widget::Custom("left".into()),
            Widget::Custom("right".into()),
        ];
        t.widgets = vec![
            box_def("left", "vertical", None, None, vec!["art"]),
            image_def("art", "art.jpg", Some(380.0), Some(480.0)),
            box_def(
                "right",
                "vertical",
                Some(8.0),
                Some(&[16.0, 10.0]),
                vec!["InputBar", "ListView"],
            ),
        ];
        // left = 480 (fixed art). right = 16 + 48 (bar) + 8 + list + 16.
        // idle (list hidden): max(480, 80) = 480 — above the 300pt config.
        let idle = measure(&t, &fit(true, false, true, true, 1, 0, 17.0));
        assert!((idle - 480.0).abs() < 1e-3, "idle got {idle}");
        // 5 results: right = 292 < 480 → the artwork still dominates.
        let typing = measure(&t, &fit(true, true, true, true, 1, 5, 17.0));
        assert!((typing - 480.0).abs() < 1e-3, "typing got {typing}");
        // 15 results: right = 712 > 480, but the chrome floor (480) and the
        // 300pt cap keep the window at 480 — the list scrolls.
        let long = measure(&t, &fit(true, true, true, true, 1, 15, 17.0));
        assert!((long - 480.0).abs() < 1e-3, "long got {long}");
    }

    /// Horizontal split with the bar in its own pane: idle height comes from
    /// the bar pane's padding + bar + gap + spacer; typing grows the window
    /// via the list pane.
    #[test]
    fn split_layout_idle_height_comes_from_bar_pane() {
        let mut t = fit_theme();
        t.window.padding = 0.0;
        t.mainbox.orientation = "horizontal".into();
        t.mainbox.gap = Some(0.0);
        t.mainbox.children = vec![
            Widget::Custom("left".into()),
            Widget::Custom("right".into()),
        ];
        t.widgets = vec![
            box_def(
                "left",
                "vertical",
                Some(12.0),
                Some(&[20.0, 24.0]),
                vec!["InputBar", "flex"],
            ),
            spacer_def("flex"),
            box_def(
                "right",
                "vertical",
                Some(8.0),
                Some(&[8.0, 10.0]),
                vec!["ListView"],
            ),
        ];
        // idle: left = 20 + 48 + 12 + 0 (spacer) + 20 = 100; right = 16.
        let idle = measure(&t, &fit(true, false, true, true, 1, 0, 17.0));
        assert!((idle - 100.0).abs() < 1e-3, "idle got {idle}");
        // 5 results: right = 8 + 204 + 8 = 220 → window grows past the bar.
        let typing = measure(&t, &fit(true, true, true, true, 1, 5, 17.0));
        assert!((typing - 220.0).abs() < 1e-3, "typing got {typing}");
    }

    /// A box with a fixed `height` is authoritative: its content (13.2pt of
    /// text) does not shrink the 100pt frame.
    #[test]
    fn fixed_height_box_is_authoritative() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def("panel", "vertical", None, None, vec!["hint"]),
            text_def("hint", 11.0),
        ];
        if let Some(WidgetDef::Box { height, .. }) = t.widgets.first_mut() {
            *height = Some(100.0);
        }
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("panel".into()),
        ];
        // 48 + 12 + 120 + 12 + 100 = 292 → + pad 20 = 312
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        assert!((h - 312.0).abs() < 1e-3, "got {h}");
    }

    /// Nested boxes: padding, gaps and content of every level add up.
    #[test]
    fn nested_boxes_sum_padding_and_gaps() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def(
                "panel",
                "vertical",
                Some(4.0),
                Some(&[6.0, 6.0]),
                vec!["strip", "hint"],
            ),
            box_def(
                "strip",
                "horizontal",
                Some(8.0),
                Some(&[2.0, 2.0]),
                vec!["btn_a", "btn_b"],
            ),
            text_def("hint", 11.0),
            button_def("btn_a", 11.0, Some(&[4.0, 8.0])),
            button_def("btn_b", 11.0, Some(&[4.0, 8.0])),
        ];
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("panel".into()),
        ];
        // strip = max(21.2, 21.2) + pad 4 = 25.2
        // panel = 25.2 + gap 4 + text 13.2 + pad 12 = 54.4
        // 48 + 12 + 120 + 12 + 54.4 = 246.4 → + pad 20 = 266.4
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        assert!((h - 266.4).abs() < 1e-3, "got {h}");
    }

    /// An image constrained by `width` only is measured by its aspect ratio
    /// (dimensions pre-resolved at theme load).
    #[test]
    fn width_only_image_uses_aspect_ratio() {
        let mut t = fit_theme();
        t.window.padding = 0.0;
        t.mainbox.orientation = "horizontal".into();
        t.mainbox.gap = Some(0.0);
        t.mainbox.children = vec![
            Widget::Custom("left".into()),
            Widget::Custom("right".into()),
        ];
        t.widgets = vec![
            box_def("left", "vertical", None, None, vec!["img"]),
            image_def("img", "/img.png", Some(100.0), None),
            box_def("right", "vertical", None, None, vec!["ListView"]),
        ];
        let reg = WidgetRegistry::from_theme(&t.widgets);
        let mut dims = HashMap::new();
        dims.insert("/img.png".to_string(), (200, 300));
        // image = 100 * 300 / 200 = 150 → window = max(150, 0) = 150
        let h = search_fit_height(&t, &reg, &fit(true, false, true, true, 1, 0, 17.0), &dims);
        assert!((h - 150.0).abs() < 1e-3, "got {h}");
    }

    /// A `w_full` image is measured at its rendered height: the aspect
    /// ratio scaled to the window width — so a wide banner no longer
    /// forces its full natural height onto the window.
    #[test]
    fn w_full_image_scales_to_window_width() {
        let mut t = fit_theme();
        t.window.padding = 0.0;
        t.window.width = Dimension::Points(100.0);
        t.mainbox.orientation = "horizontal".into();
        t.mainbox.gap = Some(0.0);
        t.mainbox.children = vec![
            Widget::Custom("left".into()),
            Widget::Custom("right".into()),
        ];
        t.widgets = vec![
            box_def("left", "vertical", None, None, vec!["img"]),
            image_def_full("img", "/img.png"),
            box_def("right", "vertical", None, None, vec!["ListView"]),
        ];
        let reg = WidgetRegistry::from_theme(&t.widgets);
        let mut dims = HashMap::new();
        dims.insert("/img.png".to_string(), (200, 300));

        // 200-wide image in a 100pt window: renders at 100 wide →
        // 100 * 300 / 200 = 150 tall, not the natural 300.
        let h = search_fit_height(&t, &reg, &fit(true, false, true, true, 1, 0, 17.0), &dims);
        assert!((h - 150.0).abs() < 1e-3, "got {h}");

        // Window wider than the image: the renderer stretches the image
        // to the full width, so the measured height follows the aspect
        // ratio at that width (400 * 300 / 200 = 600).
        t.window.width = Dimension::Points(400.0);
        let h = search_fit_height(&t, &reg, &fit(true, false, true, true, 1, 0, 17.0), &dims);
        assert!((h - 600.0).abs() < 1e-3, "got {h}");
    }

    /// Hiding the bar drops its height and one gap in custom layouts too.
    #[test]
    fn hidden_bar_drops_footer_gap_too() {
        let mut t = fit_theme();
        t.widgets = vec![
            box_def(
                "footer",
                "horizontal",
                Some(8.0),
                Some(&[8.0, 4.0]),
                vec!["hint", "flex", "btn_a", "btn_b"],
            ),
            text_def("hint", 11.0),
            spacer_def("flex"),
            button_def("btn_a", 11.0, Some(&[4.0, 8.0])),
            button_def("btn_b", 11.0, Some(&[4.0, 8.0])),
        ];
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("footer".into()),
        ];
        let with_bar = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        let without_bar = measure(&t, &fit(false, true, true, true, 1, 3, 17.0));
        // bar(48) + gap(12) = 60 less; 120 + 12 + 37.2 + 20 = 189.2
        assert!((with_bar - without_bar - 60.0).abs() < 1e-3);
        assert!((without_bar - 189.2).abs() < 1e-3, "got {without_bar}");
    }

    /// `require_input` without auto-height keeps the configured height, but
    /// grows when the fixed chrome is taller than it.
    #[test]
    fn require_input_without_auto_height_floor_is_chrome() {
        let mut t = fit_theme();
        t.window.height = Dimension::Points(80.0);
        t.widgets = vec![
            box_def(
                "footer",
                "horizontal",
                Some(8.0),
                Some(&[8.0, 4.0]),
                vec!["hint", "flex", "btn_a", "btn_b"],
            ),
            text_def("hint", 11.0),
            spacer_def("flex"),
            button_def("btn_a", 11.0, Some(&[4.0, 8.0])),
            button_def("btn_b", 11.0, Some(&[4.0, 8.0])),
        ];
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("footer".into()),
        ];
        // chrome = 48 + 12 + 37.2 = 97.2 → window = max(80, 97.2 + 20) = 117.2
        let h = measure(&t, &fit(true, true, false, true, 1, 3, 17.0));
        assert!((h - 117.2).abs() < 1e-3, "got {h}");
    }

    /// An unknown widget id is skipped: no height and no gap for it.
    #[test]
    fn unknown_widget_is_skipped() {
        let mut t = fit_theme();
        t.mainbox.children = vec![
            Widget::Builtin(BuiltinWidget::InputBar),
            Widget::Builtin(BuiltinWidget::ListView),
            Widget::Custom("missing".into()),
        ];
        let h = measure(&t, &fit(true, true, true, true, 1, 3, 17.0));
        assert!((h - 200.0).abs() < 1e-3, "got {h}");
    }

    fn example_split_state(
        t: &ThemeConfig,
        should_show_list: bool,
        filtered_len: usize,
    ) -> SearchFit {
        SearchFit {
            show_search: true,
            should_show_list,
            auto_height: true,
            require_input: true,
            ib_h: t.inputbar.height,
            columns: t.listview.columns,
            screen_w: 1400.0,
            filtered_len,
            element_font_size: t.font.size,
            screen_h: 1000.0,
        }
    }

    /// End-to-end over the bundled catppuccin split layout (bar pane + list
    /// pane, zero window padding): idle height is exactly the bar pane
    /// (padding + bar + gap + spacer); typing grows the window via the list
    /// pane.
    #[test]
    fn example_catppuccin_split_layout_measures() {
        let content = include_str!("../../examples/themes/layouts/catppuccin-split.toml");
        let mut t: ThemeConfig = toml::from_str(content).expect("should parse");
        t.listview.require_input = Some(true);
        t.window.auto_height = Some(true);
        let reg = WidgetRegistry::from_theme(&t.widgets);
        let dims = HashMap::new();

        // left pane = 20 + 46 (bar, margin 0) + 12 (gap) + 0 (spacer) + 20 = 98
        let idle = search_fit_height(&t, &reg, &example_split_state(&t, false, 0), &dims);
        assert!((idle - 98.0).abs() < 1e-3, "idle got {idle}");

        // row = 2·8 + max(24, 18) = 40; 5 rows = 232;
        // right pane = 8 + 232 + 8 = 248
        let typing = search_fit_height(&t, &reg, &example_split_state(&t, true, 5), &dims);
        assert!((typing - 248.0).abs() < 1e-3, "typing got {typing}");
    }

    /// End-to-end over the bundled graphite split layout. Its artwork is
    /// `w_full`/`h_full` — it stretches to the container and has no intrinsic
    /// height, so it never drives the window size: the list pane does. The
    /// idle window is just the bar pane; a long list grows to the configured
    /// (percent) height and stops there (the list scrolls, the art stretches
    /// to whatever height the window ends up).
    #[test]
    fn example_graphite_split_layout_stretch_art() {
        let content = include_str!("../../examples/themes/layouts/graphite-split.toml");
        let mut t: ThemeConfig = toml::from_str(content).expect("should parse");
        t.listview.require_input = Some(true);
        t.window.auto_height = Some(true);
        let reg = WidgetRegistry::from_theme(&t.widgets);
        let dims = HashMap::new();
        // "50.2%" of a 1000pt screen.
        let window_h = t.window.height.resolve(1000.0);
        assert!((window_h - 502.0).abs() < 1e-3);

        // idle: the stretch art contributes 0; right pane = 10 + 46 (bar) + 10.
        let idle = search_fit_height(&t, &reg, &example_split_state(&t, false, 0), &dims);
        assert!((idle - 66.0).abs() < 1e-3, "idle got {idle}");

        // row = 40; 15 rows = 712; right pane = 10 + 46 + 8 + 712 + 10 = 786
        // → capped at the configured 502.
        let long = search_fit_height(&t, &reg, &example_split_state(&t, true, 15), &dims);
        assert!((long - 502.0).abs() < 1e-3, "long got {long}");
    }

    #[test]
    fn scrollbar_metrics_only_when_overflowing() {
        // No overflow: no scrollbar.
        assert_eq!(scrollbar_metrics(100.0, 200.0, 0.0), None);
        assert_eq!(scrollbar_metrics(200.0, 200.0, 0.0), None);
        // Zero viewport: degenerate, no scrollbar.
        assert_eq!(scrollbar_metrics(300.0, 0.0, 0.0), None);
    }

    #[test]
    fn scrollbar_metrics_thumb_proportional_and_tracks_offset() {
        // 10 rows of 40 = 400 content in a 200 viewport: the thumb is
        // half the track: 200 * 200 / 400 = 100.
        let (y, h) = scrollbar_metrics(400.0, 200.0, 0.0).unwrap();
        assert!((y - 0.0).abs() < 1e-6, "at top, got {y}");
        assert!((h - 100.0).abs() < 1e-6, "thumb height, got {h}");

        // Half-scrolled: the thumb's top sits at the middle of its
        // travel (0.5 * (200 - 100) = 50).
        let (y, h) = scrollbar_metrics(400.0, 200.0, 100.0).unwrap();
        assert!((h - 100.0).abs() < 1e-6);
        assert!((y - 50.0).abs() < 1e-6, "mid-track, got {y}");

        // Fully scrolled: the thumb reaches the bottom of the track.
        let (y, h) = scrollbar_metrics(400.0, 200.0, 200.0).unwrap();
        assert!((y + h - 200.0).abs() < 1e-6, "at bottom, got y={y} h={h}");
    }

    #[test]
    fn scrollbar_metrics_long_list_clamps_to_min_thumb() {
        // 20,000pt of content in a 200pt viewport: the proportional
        // thumb (2pt) is far below the 24pt minimum, so it is clamped —
        // the scrollbar stays grabbable on huge lists (max_results 100+).
        let (y, h) = scrollbar_metrics(20000.0, 200.0, 0.0).unwrap();
        assert!((h - 24.0).abs() < 1e-6, "min thumb, got {h}");
        assert!((y - 0.0).abs() < 1e-6);
        // Halfway through: the clamped thumb tracks across the track.
        let (y2, h2) = scrollbar_metrics(20000.0, 200.0, 9900.0).unwrap();
        assert!((h2 - 24.0).abs() < 1e-6);
        assert!((y2 - (9900.0 / 19800.0) * (200.0 - 24.0)).abs() < 1e-6);
    }

    // ------------------------------------------------------------------
    // Fit invariants over every bundled layout (WP2-P1 of the
    // maintainability plan). These don't pin exact values — they pin the
    // contracts any correct measurer must hold, across the whole theme
    // set, so a renderer/measurer desync (the bug class behind the
    // catppuccin clipping and the w_full overestimate) fails here even
    // when no single exact-value test covers the changed theme.
    // ------------------------------------------------------------------

    fn bundled_layouts() -> Vec<ThemeConfig> {
        [
            include_str!("../../examples/themes/layouts/catppuccin-split.toml"),
            include_str!("../../examples/themes/layouts/compact.toml"),
            include_str!("../../examples/themes/layouts/graphite-split.toml"),
            include_str!("../../examples/themes/layouts/grid.toml"),
            include_str!("../../examples/themes/layouts/gruvbox-compact.toml"),
            include_str!("../../examples/themes/layouts/split.toml"),
        ]
        .iter()
        .map(|c| toml::from_str::<ThemeConfig>(c).expect("bundled layout should parse"))
        .collect()
    }

    fn layout_state(t: &ThemeConfig, auto_height: bool, show_list: bool, n: usize) -> SearchFit {
        SearchFit {
            show_search: true,
            should_show_list: show_list,
            auto_height,
            require_input: true,
            ib_h: t.inputbar.height,
            columns: t.listview.columns,
            filtered_len: n,
            element_font_size: t.font.size,
            screen_h: 1000.0,
            screen_w: 1400.0,
        }
    }

    /// Invariant 1 (floor): the window never shrinks below the fixed
    /// chrome + padding, regardless of list size or mode.
    #[test]
    fn bundled_layouts_never_shrink_below_chrome() {
        for mut t in bundled_layouts() {
            t.window.auto_height = Some(true);
            t.listview.require_input = Some(true);
            let reg = WidgetRegistry::from_theme(&t.widgets);
            let dims = HashMap::new();
            let pad2 = t.window.padding * 2.0;
            for auto in [true, false] {
                t.window.auto_height = Some(auto);
                for &n in &[0usize, 1, 5, 15, 100] {
                    let state = layout_state(&t, auto, n > 0, n);
                    let chrome = mainbox_height(&t, &reg, &state, &dims, None);
                    let h = search_fit_height(&t, &reg, &state, &dims);
                    assert!(
                        h >= chrome + pad2 - 1e-6,
                        "theme {:?}: {}pt < chrome floor {} + {} (auto={auto}, n={n})",
                        t.name,
                        h,
                        chrome,
                        pad2
                    );
                }
            }
        }
    }

    /// Invariant 2 (no clipped chrome): in a horizontal mainbox the
    /// window height must cover every pane's *fixed* content at every
    /// list size — the ListView is the only scrollable element, so its
    /// contribution is counted as zero (its slot's gap included while the
    /// list is shown). This is exactly what the v0.2.22 measurer
    /// violated for catppuccin-split (the bar pane's fixed content
    /// exceeded the window height).
    #[test]
    fn bundled_layouts_never_clip_a_pane() {
        for t in bundled_layouts() {
            if t.mainbox.orientation == "vertical" {
                continue; // vertical stacks: covered by the floor invariant
            }
            let mut th = t.clone();
            th.window.auto_height = Some(true);
            th.listview.require_input = Some(true);
            let reg = WidgetRegistry::from_theme(&th.widgets);
            let dims = HashMap::new();
            let pad2 = th.window.padding * 2.0;
            for &n in &[0usize, 1, 5, 15, 100] {
                let state = layout_state(&th, true, n > 0, n);
                let h = search_fit_height(&th, &reg, &state, &dims);
                for child in &th.mainbox.children {
                    let Some(fixed) =
                        child_height(&th, &reg, &state, &dims, (n > 0).then_some(0.0), child)
                    else {
                        continue;
                    };
                    assert!(
                        fixed + pad2 <= h + 1e-6,
                        "theme {:?}: pane {:?} fixed content {} exceeds window {} (n={n})",
                        th.name,
                        child,
                        fixed,
                        h
                    );
                }
            }
        }
    }

    /// Invariant 3 (cap): with auto_height the list grows the window but
    /// never past max(configured height, chrome floor) — long lists
    /// scroll instead of stretching the window.
    #[test]
    fn bundled_layouts_cap_at_configured_height() {
        for mut t in bundled_layouts() {
            t.window.auto_height = Some(true);
            t.listview.require_input = Some(true);
            let reg = WidgetRegistry::from_theme(&t.widgets);
            let dims = HashMap::new();
            let pad2 = t.window.padding * 2.0;
            let window_h = t.window.height.resolve(1000.0);
            let state = layout_state(&t, true, true, 1000);
            let chrome = mainbox_height(&t, &reg, &state, &dims, None);
            let h = search_fit_height(&t, &reg, &state, &dims);
            assert!(
                h <= window_h.max(chrome + pad2) + 1e-6,
                "theme {:?}: {} exceeds cap {} (chrome floor {})",
                t.name,
                h,
                window_h,
                chrome + pad2
            );
        }
    }

    /// Invariant 4 (monotone): more results never shrink the window.
    #[test]
    fn bundled_layouts_height_is_monotone_in_results() {
        for mut t in bundled_layouts() {
            t.window.auto_height = Some(true);
            t.listview.require_input = Some(true);
            let reg = WidgetRegistry::from_theme(&t.widgets);
            let dims = HashMap::new();
            let mut prev = f32::NEG_INFINITY;
            for &n in &[0usize, 1, 2, 5, 15, 50, 1000] {
                let state = layout_state(&t, true, n > 0, n);
                let h = search_fit_height(&t, &reg, &state, &dims);
                assert!(
                    h >= prev - 1e-6,
                    "theme {:?}: shrank from {} to {} between results",
                    t.name,
                    prev,
                    h
                );
                prev = h;
            }
        }
    }

    /// Invariant 5 (require_input-only exact form): without auto_height
    /// the window is the configured height (floor at the chrome) while
    /// the list is shown, and exactly the chrome while it is hidden.
    #[test]
    fn bundled_layouts_require_input_only_is_exact() {
        for mut t in bundled_layouts() {
            t.window.auto_height = Some(false);
            t.listview.require_input = Some(true);
            let reg = WidgetRegistry::from_theme(&t.widgets);
            let dims = HashMap::new();
            let pad2 = t.window.padding * 2.0;
            let window_h = t.window.height.resolve(1000.0);
            for &n in &[0usize, 1, 15, 100] {
                let state = layout_state(&t, false, n > 0, n);
                let chrome = mainbox_height(&t, &reg, &state, &dims, None);
                let h = search_fit_height(&t, &reg, &state, &dims);
                let want = if n == 0 {
                    chrome + pad2
                } else {
                    window_h.max(chrome + pad2)
                };
                assert!(
                    (h - want).abs() < 1e-3,
                    "theme {:?}: {} != {} (n={n})",
                    t.name,
                    h,
                    want
                );
            }
        }
    }
}
