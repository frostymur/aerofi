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

/// Resolve a theme font-weight spec to a GPUI [`gpui::FontWeight`].
/// Accepts CSS-style names (`"bold"`, `"semibold"`, …) or a numeric value
/// (`"700"`, `"550"`, `100`–`900`); unknown values fall back to normal.
pub(super) fn resolve_font_weight(spec: &str) -> gpui::FontWeight {
    let s = spec.trim().to_ascii_lowercase().replace(['-', ' '], "");
    match s.as_str() {
        "thin" => gpui::FontWeight::THIN,
        "extralight" => gpui::FontWeight::EXTRA_LIGHT,
        "light" => gpui::FontWeight::LIGHT,
        "normal" | "regular" => gpui::FontWeight::NORMAL,
        "medium" => gpui::FontWeight::MEDIUM,
        "semibold" => gpui::FontWeight::SEMIBOLD,
        "bold" => gpui::FontWeight::BOLD,
        "extrabold" => gpui::FontWeight::EXTRA_BOLD,
        "black" | "heavy" => gpui::FontWeight::BLACK,
        _ => s
            .parse::<f32>()
            .map(gpui::FontWeight::from)
            .unwrap_or(gpui::FontWeight::NORMAL),
    }
}

/// Common Nerd Font families, tried (in order) when the theme font is
/// missing a glyph. Icon glyphs in script rows and headers live in the
/// private-use area, so they only render if one of these is installed.
const NERD_FONT_CANDIDATES: &[&str] = &[
    "JetBrainsMono Nerd Font Mono",
    "JetBrainsMono Nerd Font",
    "Hack Nerd Font Mono",
    "Hack Nerd Font",
    "FiraCode Nerd Font",
    "CascadiaCode Nerd Font",
    "SourceCodePro Nerd Font",
    "Symbols Nerd Font",
];

/// Build the glyph-fallback cascade for the root div: the theme's own
/// `fallback` list first, then Nerd Font candidates (for icon glyphs),
/// then Apple Color Emoji and the system UI font as last resorts.
pub(super) fn font_fallback_families(theme_fallbacks: &Option<Vec<String>>) -> Vec<String> {
    let mut list = theme_fallbacks.clone().unwrap_or_default();
    for name in NERD_FONT_CANDIDATES {
        if !list.iter().any(|f| f.eq_ignore_ascii_case(name)) {
            list.push(name.to_string());
        }
    }
    if !list.iter().any(|f| f == "Apple Color Emoji") {
        list.push("Apple Color Emoji".to_string());
    }
    list.push(".AppleSystemUIFont".to_string());
    list
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
    let mut want_platform = false;
    let mut want_control = false;
    let mut want_alt = false;
    let mut want_shift = false;
    let mut key: Option<String> = None;
    for token in combo.split('+') {
        let token = token.trim().to_ascii_lowercase();
        match token.as_str() {
            "cmd" | "command" | "super" => want_platform = true,
            "ctrl" | "control" => want_control = true,
            "alt" | "option" | "opt" => want_alt = true,
            "shift" => want_shift = true,
            other if !other.is_empty() => key = Some(other.to_string()),
            _ => {}
        }
    }
    let key_matches = matches!(key.as_deref(), Some(k) if k == ks.key.to_ascii_lowercase() || (k == "r" && (ks.key == "к" || ks.key == "К")));
    let mods_match = want_platform == ks.modifiers.platform
        && want_control == ks.modifiers.control
        && want_alt == ks.modifiers.alt
        && want_shift == ks.modifiers.shift;
    key_matches && mods_match
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::FontWeight;

    #[test]
    fn resolves_named_font_weights() {
        assert_eq!(resolve_font_weight("thin"), FontWeight::THIN);
        assert_eq!(resolve_font_weight("light"), FontWeight::LIGHT);
        assert_eq!(resolve_font_weight("normal"), FontWeight::NORMAL);
        assert_eq!(resolve_font_weight("regular"), FontWeight::NORMAL);
        assert_eq!(resolve_font_weight("medium"), FontWeight::MEDIUM);
        assert_eq!(resolve_font_weight("semibold"), FontWeight::SEMIBOLD);
        assert_eq!(resolve_font_weight("bold"), FontWeight::BOLD);
        assert_eq!(resolve_font_weight("black"), FontWeight::BLACK);
        assert_eq!(resolve_font_weight("heavy"), FontWeight::BLACK);
    }

    #[test]
    fn resolves_numeric_font_weights() {
        assert_eq!(resolve_font_weight("100"), FontWeight::THIN);
        assert_eq!(resolve_font_weight("400"), FontWeight::NORMAL);
        assert_eq!(resolve_font_weight("550"), FontWeight(550.0));
        assert_eq!(resolve_font_weight("700"), FontWeight::BOLD);
        assert_eq!(resolve_font_weight("900"), FontWeight::BLACK);
    }

    #[test]
    fn resolves_hyphenated_and_case_insensitive() {
        assert_eq!(resolve_font_weight("Extra-Light"), FontWeight::EXTRA_LIGHT);
        assert_eq!(resolve_font_weight("SEMI-BOLD"), FontWeight::SEMIBOLD);
        assert_eq!(resolve_font_weight("Extra Bold"), FontWeight::EXTRA_BOLD);
    }

    #[test]
    fn falls_back_to_normal_for_unknown() {
        assert_eq!(resolve_font_weight("chunky"), FontWeight::NORMAL);
        assert_eq!(resolve_font_weight(""), FontWeight::NORMAL);
    }

    #[test]
    fn builds_fallback_cascade_with_nerd_fonts_and_emoji() {
        let list = font_fallback_families(&None);
        assert!(list.iter().any(|f| f == "JetBrainsMono Nerd Font Mono"));
        assert!(list.iter().any(|f| f == "Apple Color Emoji"));
        assert_eq!(list.last().map(String::as_str), Some(".AppleSystemUIFont"));

        let theme = vec!["SF Mono".to_string()];
        let list = font_fallback_families(&Some(theme.clone()));
        assert_eq!(list[0], "SF Mono");
        assert_eq!(
            list.iter()
                .position(|f| f == "JetBrainsMono Nerd Font Mono")
                .unwrap(),
            1
        );
    }

    #[test]
    fn fallback_cascade_dedupes_theme_entries() {
        let theme = vec![
            "JetBrainsMono Nerd Font Mono".to_string(),
            "Apple Color Emoji".to_string(),
        ];
        let list = font_fallback_families(&Some(theme));
        assert_eq!(
            list.iter()
                .filter(|f| f.as_str() == "JetBrainsMono Nerd Font Mono")
                .count(),
            1
        );
        assert_eq!(
            list.iter()
                .filter(|f| f.as_str() == "Apple Color Emoji")
                .count(),
            1
        );
    }
}
