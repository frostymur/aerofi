//! Free-standing utility functions shared across the launcher sub-modules.

use gpui::Styled;

/// Apply a full `TextStyle` to an element so its text children (including
/// `StyledText`, which inherits the parent style) render with it.
pub(super) fn apply_md_style(mut el: gpui::Div, style: &gpui::TextStyle) -> gpui::Div {
    let ts = el.text_style();
    ts.color = Some(style.color);
    ts.font_size = Some(style.font_size);
    ts.font_weight = Some(style.font_weight);
    ts.font_style = Some(style.font_style);
    el
}

/// True for a left-mouse-button click (keyboard/touch-generated clicks
/// are ignored).
pub(super) fn is_primary_click(event: &gpui::ClickEvent) -> bool {
    matches!(
        event,
        gpui::ClickEvent::Mouse(m) if m.down.button == gpui::MouseButton::Left
    )
}

/// Expand a leading `~` in a path to the user's home directory.
/// Also resolves `./` and `../` relative to the `~/.config/aerofi/` directory.
pub(super) fn expand_tilde_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{rest}", home.display());
        }
    } else if (path.starts_with("./") || path.starts_with("../"))
        && let Some(home) = dirs::home_dir()
    {
        let config_dir = home.join(".config").join("aerofi");
        return config_dir.join(path).to_string_lossy().into_owned();
    }
    path.to_string()
}

/// Render a config combo (`"cmd+shift+r"`, `"opt+space"`) in macOS glyph
/// form: modifiers in the canonical order ⌃⌥⇧⌘, then the key glyph
/// (`"⌃⇧⌘R"`, `"⌥␣"`).
pub(super) fn format_combo(combo: &str) -> String {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut cmd = false;
    let mut key = String::new();
    for token in combo.split('+') {
        let token = token.trim().to_ascii_lowercase();
        match token.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "cmd" | "command" | "super" => cmd = true,
            other if !other.is_empty() => key = other.to_string(),
            _ => {}
        }
    }
    let mut label = String::new();
    if ctrl {
        label.push('⌃');
    }
    if alt {
        label.push('⌥');
    }
    if shift {
        label.push('⇧');
    }
    if cmd {
        label.push('⌘');
    }
    label.push_str(&key_glyph(&key));
    label
}

/// macOS glyph for a key name (`"space"` -> `"␣"`, `"f12"` -> `"F12"`);
/// letters and digits are uppercased as-is.
fn key_glyph(key: &str) -> String {
    if let Some(digits) = key.strip_prefix('f')
        && let Ok(n) = digits.parse::<u8>()
        && (1..=12).contains(&n)
    {
        return format!("F{n}");
    }
    match key {
        "space" => "␣",
        "return" | "enter" => "⏎",
        "escape" | "esc" => "⎋",
        "tab" => "⇥",
        "backspace" => "⌫",
        "left" => "←",
        "right" => "→",
        "up" => "↑",
        "down" => "↓",
        _ => return key.to_uppercase(),
    }
    .to_string()
}

/// True when `combo` (e.g. "cmd+r" or "ctrl+shift+x") matches the pressed
/// keystroke: the same key and exactly the listed modifiers. Modifier
/// names: `cmd`/`command`/`super`, `ctrl`/`control`, `alt`/`option`/`opt`,
/// `shift`; the key is the remaining token (case-insensitive).
pub(super) fn combo_matches(combo: &str, ks: &gpui::Keystroke) -> bool {
    let mut want = gpui::Modifiers::default();
    let mut key: Option<String> = None;
    for token in combo.split('+') {
        let token = token.trim().to_ascii_lowercase();
        match token.as_str() {
            "cmd" | "command" | "super" => want.platform = true,
            "ctrl" | "control" => want.control = true,
            "alt" | "option" | "opt" => want.alt = true,
            "shift" => want.shift = true,
            other if !other.is_empty() => key = Some(other.to_string()),
            _ => {}
        }
    }
    matches!(key.as_deref(), Some(k) if k == ks.key.to_ascii_lowercase()) && want == ks.modifiers
}
