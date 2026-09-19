//! Rofi-compatible GUI script protocol parser.
//!
//! Scripts running in `gui` mode communicate with aerofi through
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
    /// Explicit frame marker indicating the end of a burst/frame.
    Flush,
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
    /// Set the loading indicator state (true = show spinner, false = hide).
    SetLoading(bool),
    /// Enable or disable live search (sending \0change events on typing).
    LiveSearch(bool),
    /// Highlight or mark specific row indices as active.
    SetActiveIndices(Vec<usize>),
    /// Store a data token to be passed back to the script on the next event.
    SetData(String),
    /// Display markdown preview text in the right panel.
    PreviewText(String),
    /// Load and display markdown from a file in the right panel.
    PreviewFile(String),
    /// Enable multi-selection via Tab/Shift+Tab.
    MultiSelect(bool),
    /// Enable Pango-style inline markup rendering for rows.
    MarkupRows(bool),
    /// Reload aerofi configuration immediately.
    Reload,
}

/// A single selectable (or non-selectable) row entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiRow {
    /// Visible display text (the part before any `\0` field).
    pub text: String,
    /// Unique identifier for this item (for robust selection identification).
    pub id: Option<String>,
    /// Emoji or image path for the row icon.
    pub icon: Option<String>,
    /// Small badge shown on the right side of the row.
    pub info: Option<String>,
    /// Hidden text included in fuzzy search but not rendered.
    pub meta: Option<String>,
    /// If true, the row is decorative and cannot be selected.
    pub nonselectable: bool,
    /// If true, the row is styled with an urgent accent color.
    pub urgent: bool,
    /// If true, the row is styled as currently active (e.g. connected).
    pub active: bool,
    /// If true, the row is visually disabled / grayed out.
    pub disabled: bool,
}

impl GuiRow {
    /// Create a new plain GUI row with default attributes.
    #[cfg(test)]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            id: None,
            icon: None,
            info: None,
            meta: None,
            nonselectable: false,
            urgent: false,
            active: false,
            disabled: false,
        }
    }
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

/// An event sent to a GUI script's stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiEvent {
    /// Selection of a row
    Select {
        key: String,
        index: usize,
        id: String,
        text: String,
        retv: i32,
        data: Option<String>,
        selected_ids: Vec<String>,
        selected_texts: Vec<String>,
    },
    /// Contextual action on a row
    Action {
        key: String,
        index: usize,
        id: String,
        text: String,
        retv: i32,
        data: Option<String>,
        selected_ids: Vec<String>,
        selected_texts: Vec<String>,
    },
    /// Custom input entered in the prompt
    Custom {
        key: String,
        text: String,
        retv: i32,
        data: Option<String>,
    },
    /// Input query changed in live search mode
    #[allow(dead_code)]
    Change { query: String },
}

impl GuiEvent {
    /// Format event according to the structured aerofi GUI event protocol.
    pub fn to_event_line(&self) -> String {
        match self {
            GuiEvent::Select {
                key,
                index,
                id,
                text,
                retv,
                data,
                selected_ids,
                selected_texts,
            } => {
                let data_str = data
                    .as_ref()
                    .map(|d| format!("\x1fdata:{d}"))
                    .unwrap_or_default();
                let ids_str = selected_ids.join(",");
                let texts_str = selected_texts.join(",");
                format!(
                    "\0event\x1fselect\x1fkey:{key}\x1findex:{index}\x1fid:{id}\x1ftext:{text}\x1fretv:{retv}\x1fids:{ids_str}\x1ftexts:{texts_str}{data_str}"
                )
            }
            GuiEvent::Action {
                key,
                index,
                id,
                text,
                retv,
                data,
                selected_ids,
                selected_texts,
            } => {
                let data_str = data
                    .as_ref()
                    .map(|d| format!("\x1fdata:{d}"))
                    .unwrap_or_default();
                let ids_str = selected_ids.join(",");
                let texts_str = selected_texts.join(",");
                format!(
                    "\0event\x1faction\x1fkey:{key}\x1findex:{index}\x1fid:{id}\x1ftext:{text}\x1fretv:{retv}\x1fids:{ids_str}\x1ftexts:{texts_str}{data_str}"
                )
            }
            GuiEvent::Custom {
                key,
                text,
                retv,
                data,
            } => {
                let data_str = data
                    .as_ref()
                    .map(|d| format!("\x1fdata:{d}"))
                    .unwrap_or_default();
                format!("\0event\x1fcustom\x1fkey:{key}\x1ftext:{text}\x1fretv:{retv}{data_str}")
            }
            GuiEvent::Change { query } => {
                format!("\0change\x1f{query}")
            }
        }
    }

    /// Format selection as a pipe/unit-separated line for scripts expecting `<text>\x1f<id>\x1f<index>`.
    #[cfg(test)]
    pub fn to_pipe_line(&self) -> String {
        match self {
            GuiEvent::Select {
                index, id, text, ..
            }
            | GuiEvent::Action {
                index, id, text, ..
            } => {
                format!("{text}\x1f{id}\x1f{index}")
            }
            GuiEvent::Custom { text, .. } => text.clone(),
            GuiEvent::Change { query } => format!("\0change\x1f{query}"),
        }
    }
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
/// Parse one protocol line. A line may carry several `\0`-separated
/// command fields (scripts sometimes forget the newline between two
/// commands), so a command line yields one result per parseable field;
/// a data row always yields exactly one `Row` result.
pub fn parse_gui_line(line: &str) -> Vec<GuiLineResult> {
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    if trimmed.is_empty() {
        return vec![GuiLineResult::Empty];
    }

    // Control command line: the line starts with \0 and at least one
    // \0-separated field is a known command key.
    if let Some(rest) = trimmed.strip_prefix('\0') {
        let commands: Vec<GuiCommand> = rest.split('\0').filter_map(try_parse_command).collect();
        if !commands.is_empty() {
            return commands.into_iter().map(GuiLineResult::Command).collect();
        }
    }
    // If the \0-prefixed line doesn't match a known command, treat the
    // whole line (including the leading \0) as a row — the \0 might be
    // part of field separators for a row with an empty display text.
    vec![GuiLineResult::Row(parse_row(trimmed))]
}

/// Try to parse a known control command from text after a leading `\0`.
fn try_parse_command(rest: &str) -> Option<GuiCommand> {
    if rest == "flush" {
        return Some(GuiCommand::Flush);
    }
    let (key, value) = if let Some(pair) = rest.split_once('\x1f') {
        pair
    } else {
        (rest, "")
    };
    let key = key.trim();
    let value = value.trim();

    match key {
        "flush" => Some(GuiCommand::Flush),
        "prompt" => Some(GuiCommand::SetPrompt(value.to_string())),
        "message" => Some(GuiCommand::SetMessage(value.to_string())),
        "markup-rows" if value.eq_ignore_ascii_case("true") => Some(GuiCommand::EnableMarkup),
        "no-custom" => Some(GuiCommand::NoCustom(value.eq_ignore_ascii_case("true"))),
        "keep-selection" => Some(GuiCommand::KeepSelection(value.to_string())),
        "columns" => value.parse::<usize>().ok().map(GuiCommand::SetColumns),
        "loading" => Some(GuiCommand::SetLoading(value.eq_ignore_ascii_case("true"))),
        "live-search" => Some(GuiCommand::LiveSearch(value.eq_ignore_ascii_case("true"))),
        "active" => {
            let indices = value
                .split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .collect();
            Some(GuiCommand::SetActiveIndices(indices))
        }
        "data" => Some(GuiCommand::SetData(value.to_string())),
        "preview" => Some(GuiCommand::PreviewText(value.to_string())),
        "preview-file" => Some(GuiCommand::PreviewFile(value.to_string())),
        "multi-select" => Some(GuiCommand::MultiSelect(value.eq_ignore_ascii_case("true"))),
        "markup-rows" => Some(GuiCommand::MarkupRows(value.eq_ignore_ascii_case("true"))),
        "reload" => Some(GuiCommand::Reload),
        _ => None,
    }
}

/// Parse a data row: the display text is everything before the first `\0`;
/// subsequent `\0key\x1fvalue` pairs are metadata fields.
fn parse_row(line: &str) -> GuiRow {
    let mut parts = line.split('\0');
    let text = parts.next().unwrap_or("").to_string();

    let mut id = None;
    let mut icon = None;
    let mut info = None;
    let mut meta = None;
    let mut nonselectable = false;
    let mut urgent = false;
    let mut active = false;
    let mut disabled = false;

    for field in parts {
        if let Some((key, value)) = field.split_once('\x1f') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "id" => id = Some(value.to_string()),
                "icon" => icon = Some(value.to_string()),
                "info" => info = Some(value.to_string()),
                "meta" => meta = Some(value.to_string()),
                "nonselectable" if value.eq_ignore_ascii_case("true") => nonselectable = true,
                "urgent" if value.eq_ignore_ascii_case("true") => urgent = true,
                "active" if value.eq_ignore_ascii_case("true") => active = true,
                "disabled" if value.eq_ignore_ascii_case("true") => disabled = true,
                _ => {} // unknown fields are silently ignored
            }
        }
    }

    GuiRow {
        text,
        id,
        icon,
        info,
        meta,
        nonselectable,
        urgent,
        active,
        disabled,
    }
}

impl GuiBurst {
    /// Apply parsed lines to this burst.
    pub fn from_lines(lines: &[String]) -> Self {
        let mut burst = Self::default();
        for line in lines {
            for result in parse_gui_line(line) {
                match result {
                    GuiLineResult::Command(cmd) => burst.commands.push(cmd),
                    GuiLineResult::Row(row) => burst.rows.push(row),
                    GuiLineResult::Empty => {}
                }
            }
        }
        burst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert the line yields exactly one result and return it.
    fn one(line: &str) -> GuiLineResult {
        let mut v = super::parse_gui_line(line);
        assert_eq!(v.len(), 1, "expected exactly one result, got: {v:?}");
        v.pop().unwrap()
    }

    #[test]
    fn parses_prompt_command() {
        let result = one("\0prompt\x1fSelect WiFi Network");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::SetPrompt("Select WiFi Network".to_string()))
        );
    }

    #[test]
    fn parses_message_command() {
        let result = one("\0message\x1fScanning...");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::SetMessage("Scanning...".to_string()))
        );
    }

    #[test]
    fn parses_markup_rows_command() {
        let result = one("\0markup-rows\x1ftrue");
        assert_eq!(result, GuiLineResult::Command(GuiCommand::EnableMarkup));
    }

    #[test]
    fn parses_no_custom_command() {
        assert_eq!(
            one("\0no-custom\x1ftrue"),
            GuiLineResult::Command(GuiCommand::NoCustom(true))
        );
        assert_eq!(
            one("\0no-custom\x1ffalse"),
            GuiLineResult::Command(GuiCommand::NoCustom(false))
        );
    }

    #[test]
    fn parses_keep_selection_command() {
        let result = one("\0keep-selection\x1fNetwork A");
        assert_eq!(
            result,
            GuiLineResult::Command(GuiCommand::KeepSelection("Network A".to_string()))
        );
    }

    #[test]
    fn parses_columns_command() {
        let result = one("\0columns\x1f3");
        assert_eq!(result, GuiLineResult::Command(GuiCommand::SetColumns(3)));
    }

    #[test]
    fn parses_columns_invalid_returns_none() {
        // Unknown command (invalid number) — falls through to row parsing.
        let result = one("\0columns\x1fabc");
        // \0columns is not a valid row prefix either — but our parser
        // treats it as a row with text = "" and a field "columns\x1fabc"
        // which is unknown. The row text will be empty.
        assert!(matches!(result, GuiLineResult::Row(_)));
    }

    #[test]
    fn parses_unknown_command_as_row() {
        let result = one("\0unknown-cmd\x1fvalue");
        // Unknown \0-prefixed line becomes a row.
        assert!(matches!(result, GuiLineResult::Row(_)));
    }

    #[test]
    fn parses_plain_row() {
        let result = one("Firefox");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Firefox".to_string(),
                id: None,
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
                urgent: false,
                active: false,
                disabled: false,
            })
        );
    }

    #[test]
    fn parses_row_with_icon() {
        let result = one("Home Network\0icon\x1f📶");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Home Network".to_string(),
                id: None,
                icon: Some("📶".to_string()),
                info: None,
                meta: None,
                nonselectable: false,
                urgent: false,
                active: false,
                disabled: false,
            })
        );
    }

    #[test]
    fn parses_row_with_all_fields() {
        let result = one(
            "Network A\0id\x1fnet_a\0icon\x1f📶\0info\x1fWPA2\0meta\x1fsecure network\0nonselectable\x1ftrue\0urgent\x1ftrue\0active\x1ftrue\0disabled\x1ftrue",
        );
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Network A".to_string(),
                id: Some("net_a".to_string()),
                icon: Some("📶".to_string()),
                info: Some("WPA2".to_string()),
                meta: Some("secure network".to_string()),
                nonselectable: true,
                urgent: true,
                active: true,
                disabled: true,
            })
        );
    }

    #[test]
    fn parses_row_with_info_only() {
        let result = one("Item\0info\x1fActive");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Item".to_string(),
                id: None,
                icon: None,
                info: Some("Active".to_string()),
                meta: None,
                nonselectable: false,
                urgent: false,
                active: false,
                disabled: false,
            })
        );
    }

    #[test]
    fn parses_row_nonselectable_false() {
        let result = one("Item\0nonselectable\x1ffalse");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Item".to_string(),
                id: None,
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
                urgent: false,
                active: false,
                disabled: false,
            })
        );
    }

    #[test]
    fn empty_line_returns_empty() {
        assert_eq!(parse_gui_line(""), vec![GuiLineResult::Empty]);
        assert_eq!(parse_gui_line("\n"), vec![GuiLineResult::Empty]);
        assert_eq!(parse_gui_line("\r\n"), vec![GuiLineResult::Empty]);
    }

    #[test]
    fn gui_burst_from_lines() {
        let lines = vec![
            "\0prompt\x1fPick one".to_string(),
            "Item A\0icon\x1f🅰️".to_string(),
            "Item B\0icon\x1f🅱️\0info\x1fnew".to_string(),
            "".to_string(),
            "\0message\x1fHello".to_string(),
            "\0flush".to_string(),
        ];
        let burst = GuiBurst::from_lines(&lines);
        assert_eq!(burst.commands.len(), 3);
        assert_eq!(burst.rows.len(), 2);
        assert_eq!(
            burst.commands[0],
            GuiCommand::SetPrompt("Pick one".to_string())
        );
        assert_eq!(
            burst.commands[1],
            GuiCommand::SetMessage("Hello".to_string())
        );
        assert_eq!(burst.commands[2], GuiCommand::Flush);
        assert_eq!(burst.rows[0].text, "Item A");
        assert_eq!(burst.rows[1].text, "Item B");
        assert_eq!(burst.rows[1].info, Some("new".to_string()));
    }

    #[test]
    fn row_with_unknown_fields_ignored() {
        let result = one("Test\0custom_field\x1fvalue\0icon\x1f🔥");
        assert_eq!(
            result,
            GuiLineResult::Row(GuiRow {
                text: "Test".to_string(),
                id: None,
                icon: Some("🔥".to_string()),
                info: None,
                meta: None,
                nonselectable: false,
                urgent: false,
                active: false,
                disabled: false,
            })
        );
    }

    #[test]
    fn parses_flush_loading_live_search_and_active() {
        assert_eq!(one("\0flush"), GuiLineResult::Command(GuiCommand::Flush));
        assert_eq!(
            one("\0flush\x1ftrue"),
            GuiLineResult::Command(GuiCommand::Flush)
        );
        assert_eq!(
            one("\0loading\x1ftrue"),
            GuiLineResult::Command(GuiCommand::SetLoading(true))
        );
        assert_eq!(
            one("\0loading\x1ffalse"),
            GuiLineResult::Command(GuiCommand::SetLoading(false))
        );
        assert_eq!(
            one("\0live-search\x1ftrue"),
            GuiLineResult::Command(GuiCommand::LiveSearch(true))
        );
        assert_eq!(
            one("\0active\x1f0,2,5"),
            GuiLineResult::Command(GuiCommand::SetActiveIndices(vec![0, 2, 5]))
        );
    }

    #[test]
    fn formats_gui_events() {
        let select_ev = GuiEvent::Select {
            key: "enter".to_string(),
            index: 2,
            id: "wifi_home".to_string(),
            text: "Home Network".to_string(),
            retv: 1,
            data: Some("my_state".to_string()),
            selected_ids: vec!["wifi_home".to_string()],
            selected_texts: vec!["Home Network".to_string()],
        };
        assert_eq!(
            select_ev.to_event_line(),
            "\0event\x1fselect\x1fkey:enter\x1findex:2\x1fid:wifi_home\x1ftext:Home Network\x1fretv:1\x1fids:wifi_home\x1ftexts:Home Network\x1fdata:my_state"
        );
        assert_eq!(select_ev.to_pipe_line(), "Home Network\x1fwifi_home\x1f2");

        let action_ev = GuiEvent::Action {
            key: "ctrl+d".to_string(),
            index: 2,
            id: "wifi_home".to_string(),
            text: "Home Network".to_string(),
            retv: 12,
            data: None,
            selected_ids: vec!["wifi_home".to_string()],
            selected_texts: vec!["Home Network".to_string()],
        };
        assert_eq!(
            action_ev.to_event_line(),
            "\0event\x1faction\x1fkey:ctrl+d\x1findex:2\x1fid:wifi_home\x1ftext:Home Network\x1fretv:12\x1fids:wifi_home\x1ftexts:Home Network"
        );

        let custom_ev = GuiEvent::Custom {
            key: "enter".to_string(),
            text: "my_typed_custom_input".to_string(),
            retv: 2,
            data: None,
        };
        assert_eq!(
            custom_ev.to_event_line(),
            "\0event\x1fcustom\x1fkey:enter\x1ftext:my_typed_custom_input\x1fretv:2"
        );
        assert_eq!(custom_ev.to_pipe_line(), "my_typed_custom_input");

        let change_ev = GuiEvent::Change {
            query: "query text".to_string(),
        };
        assert_eq!(change_ev.to_event_line(), "\0change\x1fquery text");

        assert_eq!(
            one("\0reload\x1ftrue"),
            GuiLineResult::Command(GuiCommand::Reload)
        );
        assert_eq!(one("\0reload"), GuiLineResult::Command(GuiCommand::Reload));
    }

    #[test]
    fn multiple_commands_on_one_line() {
        // A script that forgets the newline between two commands emits
        // both on one line: both must be parsed, value must not leak.
        let results = parse_gui_line("\0prompt\x1fSearch emojis\u{2026}\0columns\x1f8");
        assert_eq!(
            results,
            vec![
                GuiLineResult::Command(GuiCommand::SetPrompt("Search emojis\u{2026}".to_string())),
                GuiLineResult::Command(GuiCommand::SetColumns(8))
            ]
        );
    }
}
