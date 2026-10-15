//! Widget state conditions: the `visible_when` / `selected_when`
//! predicates themes can attach to widgets.
//!
//! Grammar (deliberately minimal — one comparison, no boolean
//! operators):
//!
//! ```text
//! <key> == <value>
//!
//!   key:    mode | query | list
//!   mode:   any mode or plugin name, or `all` (the unfiltered state)
//!   query:  empty | nonempty
//!   list:   empty | nonempty
//! ```
//!
//! The renderer and the auto_height measurer both evaluate these
//! against the same snapshot of launcher state (see
//! [`WidgetCondition::eval`]), so a hidden widget is never rendered
//! *and* never measured — the two-sides contract holds by
//! construction.
//!
//! Examples:
//! ```toml
//! # Highlight the active mode in a switcher bar.
//! [widgets.mode_apps]
//! text = "Apps"
//! action = "mode:apps"
//! selected_when = "mode == apps"
//!
//! [widgets.mode_apps.selected]
//! background = "$accent"
//! color = "$surface"
//!
//! # A hint that only shows before the user typed anything.
//! [widgets.hint]
//! type = "text"
//! text = "start typing"
//! visible_when = "query == empty"
//! ```

/// The state a condition compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKey {
    /// The active mode label: a `[modes]` key, a plugin name, or
    /// `all` when no mode is active.
    Mode,
    /// Whether the input query is empty.
    Query,
    /// Whether the results list is empty.
    List,
}

/// A parsed widget condition: one comparison against one state key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetCondition {
    pub key: StateKey,
    pub value: String,
}

/// The values `query` / `list` accept.
const QUERY_LIST_VALUES: [&str; 2] = ["empty", "nonempty"];

/// Parse a `visible_when` / `selected_when` string. Returns `None` for
/// anything outside the grammar (the caller warns and ignores the
/// condition, matching the theme system's warn-and-continue policy).
pub fn parse_condition(input: &str) -> Option<WidgetCondition> {
    let (key, value) = input.split_once("==")?;
    let key = key.trim();
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let key = match key {
        "mode" => StateKey::Mode,
        "query" => StateKey::Query,
        "list" => StateKey::List,
        _ => return None,
    };
    let value = match key {
        // mode names are user-defined keys; strip optional single quotes
        StateKey::Mode => value.trim_matches(|c| c == '\'' || c == '"').to_string(),
        StateKey::Query | StateKey::List => {
            if !QUERY_LIST_VALUES.contains(&value) {
                return None;
            }
            value.to_string()
        }
    };
    if value.is_empty() {
        return None;
    }
    Some(WidgetCondition { key, value })
}

impl WidgetCondition {
    /// Evaluate against a launcher-state snapshot.
    ///
    /// - `active_mode`: the mode label, `all` when unfiltered
    /// - `query_empty` / `list_empty`: the current query/list state
    pub fn eval(&self, active_mode: &str, query_empty: bool, list_empty: bool) -> bool {
        match self.key {
            StateKey::Mode => self.value == active_mode,
            StateKey::Query => match self.value.as_str() {
                "empty" => query_empty,
                "nonempty" => !query_empty,
                _ => false,
            },
            StateKey::List => match self.value.as_str() {
                "empty" => list_empty,
                "nonempty" => !list_empty,
                _ => false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_conditions() {
        let c = parse_condition("mode == apps").unwrap();
        assert_eq!(c.key, StateKey::Mode);
        assert_eq!(c.value, "apps");
        assert!(c.eval("apps", true, true));
        assert!(!c.eval("all", true, true));
        // A mode key with a space needs quotes.
        let c = parse_condition("mode == 'my mode'").unwrap();
        assert_eq!(c.value, "my mode");
        // Plugin names with dashes work unquoted.
        let c = parse_condition("mode == file-search").unwrap();
        assert!(c.eval("file-search", false, false));
        // `all` is just the unfiltered label.
        let c = parse_condition("mode == all").unwrap();
        assert!(c.eval("all", true, false));
        assert!(!c.eval("apps", true, false));
    }

    #[test]
    fn parses_query_and_list_conditions() {
        for cond in [
            "query == empty",
            "query == nonempty",
            "list == empty",
            "list == nonempty",
        ] {
            assert!(parse_condition(cond).is_some(), "{cond}");
        }
        let c = parse_condition("query == nonempty").unwrap();
        assert!(!c.eval("all", true, true));
        assert!(c.eval("all", false, true));
        let c = parse_condition("list == empty").unwrap();
        assert!(c.eval("all", true, true));
        assert!(!c.eval("all", false, false));
    }

    #[test]
    fn rejects_out_of_grammar_input() {
        assert!(parse_condition("").is_none());
        assert!(parse_condition("mode").is_none());
        assert!(parse_condition("mode apps").is_none());
        assert!(parse_condition("theme == dark").is_none());
        assert!(parse_condition("mode ==").is_none());
        assert!(parse_condition("query == short").is_none());
        assert!(parse_condition("list == 5").is_none());
        assert!(parse_condition("mode == ''").is_none());
    }
}
