//! Rofi-compatible GUI script protocol parser.
//!
//! Scripts running in `gui` mode communicate with AeroFi through
//! structured stdout lines.  Control commands start with `\0` and use
//! `\x1f` (ASCII unit-separator) to delimit key/value pairs.  Data rows
//! are plain text optionally followed by `\0`-delimited metadata fields.
//!
//! # Protocol
//!
//! ## Control commands
//! ```text
//! \0prompt\x1fSelect network
//! \0message\x1fScanning…
//! \0no-custom\x1ftrue
//! \0keep-selection\x1fNetwork A
//! \0columns\x1f3
//! ```
//!
//! ## Data rows
//! ```text
//! Network A\0icon\x1f📶\0info\x1fWPA2\0meta\x1fhidden search text
//! Header line\0nonselectable\x1ftrue
//! ```

/// A parsed control command from a line starting with `\0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiCommand {
    /// Override the input bar placeholder text.
    SetPrompt(String),
    /// Show a status/info message (above or below the list).
    SetMessage(String),
    /// Enable rich-text (Pango-style) parsing in row names (future).
    EnableMarkup,
    /// If true, only listed items are selectable (no free-form typing).
    NoCustom(bool),
    /// Pre-select a row by its display text after a list refresh.
    KeepSelection(String),
    /// Override the grid column count for this step.
    SetColumns(usize),
}

/// A single selectable (or non-selectable) row entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiRow {
    /// Visible display text (the part before any `\0` field).
    pub text: String,
    /// Emoji or image path for the row icon.
    pub icon: Option<String>,
    /// Small badge shown on the right side of the row.
    pub info: Option<String>,
    /// Hidden text included in fuzzy search but not rendered.
    pub meta: Option<String>,
    /// If true, the row is decorative and cannot be selected.
    pub nonselectable: bool,
}

/// The result of parsing a single stdout line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiLineResult {
    /// A control command (line started with `\0` and matched a known key).
    Command(GuiCommand),
    /// A data row (possibly with metadata fields).
    Row(GuiRow),
    /// An empty or whitespace-only line — skip.
    Empty,
}

/// A "burst" of output: all commands and rows from one read cycle.
#[derive(Debug, Clone, Default)]
pub struct GuiBurst {
    /// Control commands received in this burst.
    pub commands: Vec<GuiCommand>,
    /// Data rows received in this burst.
    pub rows: Vec<GuiRow>,
}

/// Parse a single line from a GUI script's stdout.
///
/// Lines starting with `\0` are control commands; all others are data rows.
/// Empty/whitespace-only lines are `GuiLineResult::Empty`.
pub fn parse_gui_line(line: &str) -> GuiLineResult {
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    if trimmed.is_empty() {
        return GuiLineResult::Empty;
    }

    // Control command: the line starts with \0 and the first field is a
    // known command key.
    if let Some(rest) = trimmed.strip_prefix('\0') {
        if let Some(cmd) = try_parse_command(rest) {
            return GuiLineResult::Command(cmd);
        }
        // If the \0-prefixed line doesn't match a known command, treat the
        // whole line (including the leading \0) as a row — the \0 might be
        // part of field separators for a row with an empty display text.
    }

    GuiLineResult::Row(parse_row(trimmed))
}

/// Try to parse a known control command from text after a leading `\0`.
fn try_parse_command(rest: &str) -> Option<GuiCommand> {
    // Split on \x1f (unit separator): "prompt\x1fSelect network"
    let (key, value) = rest.split_once('\x1f')?;
    let key = key.trim();
    let value = value.trim();

    match key {
        "prompt" => Some(GuiCommand::SetPrompt(value.to_string())),
        "message" => Some(GuiCommand::SetMessage(value.to_string())),
        "markup-rows" if value.eq_ignore_ascii_case("true") => Some(GuiCommand::EnableMarkup),
        "no-custom" => Some(GuiCommand::NoCustom(value.eq_ignore_ascii_case("true"))),
        "keep-selection" => Some(GuiCommand::KeepSelection(value.to_string())),
        "columns" => value.parse::<usize>().ok().map(GuiCommand::SetColumns),
        _ => None,
    }
}

/// Parse a data row: the display text is everything before the first `\0`;
/// subsequent `\0key\x1fvalue` pairs are metadata fields.
fn parse_row(line: &str) -> GuiRow {
    let mut parts = line.split('\0');
    let text = parts.next().unwrap_or("").to_string();

    let mut icon = None;
    let mut info = None;
    let mut meta = None;
    let mut nonselectable = false;

    for field in parts {
        if let Some((key, value)) = field.split_once('\x1f') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "icon" => icon = Some(value.to_string()),
                "info" => info = Some(value.to_string()),
                "meta" => meta = Some(value.to_string()),
                "nonselectable" if value.eq_ignore_ascii_case("true") => nonselectable = true,
                _ => {} // unknown fields are silently ignored
            }
        }
    }

    GuiRow {
        text,
        icon,
        info,
        meta,
        nonselectable,
    }
}

impl GuiBurst {
    /// Apply parsed lines to this burst.
    pub fn from_lines(lines: &[String]) -> Self {
        let mut burst = Self::default();
        for line in lines {
            match parse_gui_line(line) {
                GuiLineResult::Command(cmd) => burst.commands.push(cmd),
                GuiLineResult::Row(row) => burst.rows.push(row),
                GuiLineResult::Empty => {}
            }
        }
        burst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_command() {
        let result = parse_gui_line("\0prompt\x1fSelect WiFi Network");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::SetPrompt(
                "Select WiFi Network".to_string()
            ))
        );
    }

    #[test]
    fn parses_message_command() {
        let result = parse_gui_line("\0message\x1fScanning...");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::SetMessage("Scanning...".to_string()))
        );
    }

    #[test]
    fn parses_markup_rows_command() {
        let result = parse_gui_line("\0markup-rows\x1ftrue");
        assert_eq!(result, GuiLineResult::Command(GuiCommand::EnableMarkup));
    }

    #[test]
    fn parses_no_custom_command() {
        assert_eq!(
            parse_gui_line("\0no-custom\x1ftrue"),
            GuiLineResult::Command(GuiCommand::NoCustom(true))
        );
        assert_eq!(
            parse_gui_line("\0no-custom\x1ffalse"),
            GuiLineResult::Command(GuiCommand::NoCustom(false))
        );
    }

    #[test]
    fn parses_keep_selection_command() {
        let result = parse_gui_line("\0keep-selection\x1fNetwork A");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::KeepSelection("Network A".to_string()))
        );
    }

    #[test]
    fn parses_columns_command() {
        let result = parse_gui_line("\0columns\x1f3");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::SetColumns(3))
        );
    }

    #[test]
    fn parses_columns_invalid_returns_none() {
        // Unknown command (invalid number) — falls through to row parsing.
        let result = parse_gui_line("\0columns\x1fabc");
        // \0columns is not a valid row prefix either — but our parser
        // treats it as a row with text = "" and a field "columns\x1fabc"
        // which is unknown. The row text will be empty.
        assert!(matches!(result, GuiLineResult::Row(_)));
    }

    #[test]
    fn parses_unknown_command_as_row() {
        let result = parse_gui_line("\0unknown-cmd\x1fvalue");
        // Unknown \0-prefixed line becomes a row.
        assert!(matches!(result, GuiLineResult::Row(_)));
    }

    #[test]
    fn parses_plain_row() {
        let result = parse_gui_line("Firefox");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Firefox".to_string(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            })
        );
    }

    #[test]
    fn parses_row_with_icon() {
        let result = parse_gui_line("Home Network\0icon\x1f📶");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Home Network".to_string(),
                icon: Some("📶".to_string()),
                info: None,
                meta: None,
                nonselectable: false,
            })
        );
    }

    #[test]
    fn parses_row_with_all_fields() {
        let result =
            parse_gui_line("Network A\0icon\x1f📶\0info\x1fWPA2\0meta\x1fsecure network\0nonselectable\x1ftrue");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Network A".to_string(),
                icon: Some("📶".to_string()),
                info: Some("WPA2".to_string()),
                meta: Some("secure network".to_string()),
                nonselectable: true,
            })
        );
    }

    #[test]
    fn parses_row_with_info_only() {
        let result = parse_gui_line("Item\0info\x1fActive");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Item".to_string(),
                icon: None,
                info: Some("Active".to_string()),
                meta: None,
                nonselectable: false,
            })
        );
    }

    #[test]
    fn parses_row_nonselectable_false() {
        let result = parse_gui_line("Item\0nonselectable\x1ffalse");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Item".to_string(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            })
        );
    }

    #[test]
    fn empty_line_returns_empty() {
        assert_eq!(parse_gui_line(""), GuiLineResult::Empty);
        assert_eq!(parse_gui_line("\n"), GuiLineResult::Empty);
        assert_eq!(parse_gui_line("\r\n"), GuiLineResult::Empty);
    }

    #[test]
    fn gui_burst_from_lines() {
        let lines = vec![
            "\0prompt\x1fPick one".to_string(),
            "Item A\0icon\x1f🅰️".to_string(),
            "Item B\0icon\x1f🅱️\0info\x1fnew".to_string(),
            "".to_string(),
            "\0message\x1fHello".to_string(),
        ];
        let burst = GuiBurst::from_lines(&lines);
        assert_eq!(burst.commands.len(), 2);
        assert_eq!(burst.rows.len(), 2);
        assert_eq!(
            burst.commands[0],
            GuiCommand::SetPrompt("Pick one".to_string())
        );
        assert_eq!(
            burst.commands[1],
            GuiCommand::SetMessage("Hello".to_string())
        );
        assert_eq!(burst.rows[0].text, "Item A");
        assert_eq!(burst.rows[1].text, "Item B");
        assert_eq!(burst.rows[1].info, Some("new".to_string()));
    }

    #[test]
    fn row_with_unknown_fields_ignored() {
        let result = parse_gui_line("Test\0custom_field\x1fvalue\0icon\x1f🔥");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Test".to_string(),
                icon: Some("🔥".to_string()),
                info: None,
                meta: None,
                nonselectable: false,
            })
        );
    }
}
