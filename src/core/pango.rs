use std::ops::Range;

/// Parsed emphasis for a span of plain text.
///
/// Spec-level on purpose: no GPUI types — the UI maps this to
/// `gpui::HighlightStyle` at the render boundary. Colors are packed
/// `0xRRGGBBAA` (the same convention `core::theme::parse_hex_color`
/// uses); `None` means "theme default" / "no background".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PangoStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    /// Foreground color, packed 0xRRGGBBAA (None = theme default).
    pub color: Option<u32>,
    /// Background color, packed 0xRRGGBBAA (None = no background).
    pub background: Option<u32>,
}

/// Parse lightweight Pango-style markup and return the plain text along
/// with the emphasis of each span ([`PangoStyle`], GPUI-free).
pub fn parse_pango(input: &str) -> (String, Vec<(Range<usize>, PangoStyle)>) {
    let mut plain_text = String::with_capacity(input.len());
    let mut highlights = Vec::new();
    let mut stack: Vec<(usize, PangoStyle)> = Vec::new();

    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag_content = String::new();
            let mut is_tag = false;
            while let Some(&next_c) = chars.peek() {
                if next_c == '>' {
                    chars.next();
                    is_tag = true;
                    break;
                }
                tag_content.push(next_c);
                chars.next();
            }

            if !is_tag {
                // Was not a closed tag, just append raw
                plain_text.push('<');
                plain_text.push_str(&tag_content);
                continue;
            }

            let tag_content = tag_content.trim();
            if tag_content.starts_with('/') {
                // Closing tag
                if let Some((start_idx, style)) = stack.pop() {
                    let end_idx = plain_text.len();
                    if end_idx > start_idx {
                        highlights.push((start_idx..end_idx, style));
                    }
                }
            } else {
                // Opening tag
                let mut parts = tag_content.splitn(2, ' ');
                let tag_name = parts.next().unwrap_or("").to_lowercase();
                let attrs_str = parts.next().unwrap_or("");

                let mut style = PangoStyle::default();
                let mut apply = false;

                match tag_name.as_str() {
                    "b" => {
                        style.bold = true;
                        apply = true;
                    }
                    "i" => {
                        style.italic = true;
                        apply = true;
                    }
                    "u" => {
                        style.underline = true;
                        apply = true;
                    }
                    "s" => {
                        style.strikethrough = true;
                        apply = true;
                    }
                    "span" | "font" => {
                        apply = true;
                        // Parse simple attributes: key="value" or key='value'
                        let mut attrs = attrs_str;
                        while let Some(eq_idx) = attrs.find('=') {
                            let key = attrs[..eq_idx].trim().to_lowercase();
                            attrs = attrs[eq_idx + 1..].trim_start();
                            if attrs.is_empty() {
                                break;
                            }

                            let quote = attrs.chars().next().unwrap();
                            if quote == '"' || quote == '\'' {
                                attrs = &attrs[1..];
                                if let Some(end_idx) = attrs.find(quote) {
                                    let value = &attrs[..end_idx];
                                    attrs = &attrs[end_idx + 1..];

                                    match key.as_str() {
                                        "foreground" | "color" => {
                                            style.color = parse_hex_color(value);
                                        }
                                        "background" | "bg_color" => {
                                            style.background = parse_hex_color(value);
                                        }
                                        "weight" => {
                                            if value.eq_ignore_ascii_case("bold") {
                                                style.bold = true;
                                            }
                                        }
                                        "style" if value.eq_ignore_ascii_case("italic") => {
                                            style.italic = true;
                                        }
                                        _ => {}
                                    }
                                }
                            } else {
                                // Unquoted attribute value, skip to space
                                if let Some(space_idx) = attrs.find(' ') {
                                    attrs = &attrs[space_idx + 1..];
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    _ => {} // Unknown tag, just ignore and don't push to stack
                }

                if apply {
                    stack.push((plain_text.len(), style));
                }
            }
        } else {
            // Unescape common HTML entities
            if c == '&' {
                let mut entity = String::new();
                let mut is_entity = false;
                let mut lookahead = chars.clone();
                for _ in 0..5 {
                    if let Some(&next_c) = lookahead.peek() {
                        lookahead.next();
                        if next_c == ';' {
                            is_entity = true;
                            break;
                        }
                        entity.push(next_c);
                    } else {
                        break;
                    }
                }

                if is_entity {
                    let decoded = match entity.as_str() {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        _ => None,
                    };

                    if let Some(dec_c) = decoded {
                        plain_text.push(dec_c);
                        for _ in 0..entity.len() + 1 {
                            chars.next(); // Consume entity + semicolon
                        }
                        continue;
                    }
                }
            }
            plain_text.push(c);
        }
    }

    // Close any unclosed tags
    while let Some((start_idx, style)) = stack.pop() {
        let end_idx = plain_text.len();
        if end_idx > start_idx {
            highlights.push((start_idx..end_idx, style));
        }
    }

    (plain_text, highlights)
}

/// Parse a `#RRGGBB` / `#RRGGBBAA` hex color into packed 0xRRGGBBAA.
fn parse_hex_color(hex: &str) -> Option<u32> {
    let hex = hex.trim_start_matches('#');
    let len = hex.len();
    if len != 6 && len != 8 {
        return None;
    }
    let r = u32::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u32::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u32::from_str_radix(&hex[4..6], 16).ok()?;
    let a = if len == 8 {
        u32::from_str_radix(&hex[6..8], 16).ok()?
    } else {
        255
    };
    Some((r << 24) | (g << 16) | (b << 8) | a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pango_basic() {
        let (text, highlights) = parse_pango("Hello <b>world</b>!");
        assert_eq!(text, "Hello world!");
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].0, 6..11);
        assert!(highlights[0].1.bold);
    }

    #[test]
    fn test_parse_pango_span() {
        let (text, highlights) = parse_pango("Error: <span foreground='#ff0000'>Critical</span>");
        assert_eq!(text, "Error: Critical");
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].0, 7..15);
        assert!(highlights[0].1.color.is_some());
    }

    #[test]
    fn test_parse_pango_nested() {
        let (text, highlights) = parse_pango("<i><b>BoldItalic</b></i>");
        assert_eq!(text, "BoldItalic");
        assert_eq!(highlights.len(), 2);
    }

    #[test]
    fn test_parse_entities() {
        let (text, _) = parse_pango("1 &lt; 2 &amp; 3 &gt; 1");
        assert_eq!(text, "1 < 2 & 3 > 1");
    }
}
