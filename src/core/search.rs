//! nucleo-matcher wrapper: ranks targets (by display name or configured
//! aliases) against a filter query, boosted by frecency.

use std::collections::HashMap;

use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::core::history::History;
use crate::core::item::Target;

use gpui::SharedString;

/// A reusable fuzzy matcher. Holds the nucleo `Matcher` (it allocates a
/// working set up front, so it is built once and reused), a reverse alias
/// index (target display name -> the aliases that point to it), and the
/// UTF-32 scratch buffers. Everything is allocated once in `new` and
/// reused across `filter_and_rank()` calls.
pub struct SearchIndex {
    matcher: Matcher,
    aliases_by_target: HashMap<SharedString, Vec<SharedString>>,
    /// Pinned target entries (display name or full path), in the order the
    /// user listed them. Pinned matches outrank frecency and fuzzy score.
    pinned: Vec<SharedString>,
    needle_buf: Vec<char>,
    hay_buf: Vec<char>,
    /// Reused scratch buffer for (is_pinned, pinned_order, score, index) —
    /// avoids a per-keystroke heap allocation.
    scored_buf: Vec<(bool, u32, u32, usize)>,
    /// Reused buffer for the lowercased query string.
    lower_query_buf: String,
}

impl SearchIndex {
    /// Build an index that also matches the given `aliases` (alias ->
    /// target display name) and pins the given `pinned` entries. Alias
    /// values must equal the target's display name exactly; aliases pointing
    /// to a target that is not in the searched list simply never match.
    ///
    /// A `pinned` entry matches a target by display name (case-insensitive)
    /// or by full path. Pinned matches are ranked above everything else, in
    /// the order the entries were listed.
    pub fn new(aliases: &HashMap<String, String>, pinned: &[String]) -> Self {
        let mut aliases_by_target: HashMap<SharedString, Vec<SharedString>> = HashMap::new();
        for (alias, target) in aliases {
            aliases_by_target
                .entry(SharedString::from(target.clone()))
                .or_default()
                .push(SharedString::from(alias.clone()));
        }
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            aliases_by_target,
            pinned: pinned
                .iter()
                .map(|p| SharedString::from(p.clone()))
                .collect(),
            needle_buf: Vec::new(),
            hay_buf: Vec::new(),
            scored_buf: Vec::new(),
            lower_query_buf: String::new(),
        }
    }

    /// Rank `targets` against `query`, boosted by the frecency scores
    /// from `history`, and return all matching ones as a ready-to-render
    /// `Vec<Target>`, best match first.
    ///
    /// - Empty query: every target matches with a zero fuzzy score, so
    ///   the order is by frecency, descending. Targets with frecency 0
    ///   keep their original order (stable sort).
    /// - Non-empty query: a target matches when its display name or one
    ///   of its aliases fuzzy-matches the query; the frecency score is
    ///   added to the best fuzzy score before ranking.
    ///
    /// Ties keep the original order.
    pub fn search(
        &mut self,
        query: &str,
        targets: &[Target],
        history: &History,
        out_filtered: &mut Vec<usize>,
    ) {
        // Reuse all scratch buffers to avoid heap allocations every keystroke.
        self.needle_buf.clear();
        self.hay_buf.clear();
        self.scored_buf.clear();
        out_filtered.clear();

        // nucleo lowercases the haystack when matching (ignore_case) but
        // compares needle characters verbatim. An uppercase needle char can
        // then pass the prefilter (which matches it case-sensitively) and
        // still fail inside the optimal matcher, which panics with
        // "should have been caught by prefilter". Lowercasing the query
        // keeps the needle consistent with the normalized haystack.
        self.lower_query_buf.clear();
        self.lower_query_buf
            .extend(query.chars().flat_map(|c| c.to_lowercase()));
        let query = self.lower_query_buf.as_str();

        let needle = Utf32Str::new(query, &mut self.needle_buf);
        let frecency_map = history.calculate_frecency_map();
        for (i, target) in targets.iter().enumerate() {
            let name = target.name();
            let aliases = self.aliases_by_target.get(name);

            let mut fuzzy: Option<u16> = if query.is_empty() {
                Some(0)
            } else {
                let hay = Utf32Str::new(name, &mut self.hay_buf);
                self.matcher.fuzzy_match(hay, needle)
            };
            if !query.is_empty()
                && let Some(list) = aliases
            {
                for alias in list {
                    let hay = Utf32Str::new(alias, &mut self.hay_buf);
                    if let Some(score) = self.matcher.fuzzy_match(hay, needle) {
                        fuzzy = Some(fuzzy.map_or(score, |b| b.max(score)));
                    }
                }
            }

            let Some(fuzzy_score) = fuzzy else {
                continue;
            };
            let frecency = frecency_map.get(target.identifier()).copied().unwrap_or(0);
            let score = u32::from(fuzzy_score) + frecency;
            // Pinned entries match by display name (case-insensitive) or by
            // full path. The list is short, so a linear scan is cheap and
            // allocation-free; `position` also yields the user's ordering.
            let pinned_order = self.pinned.iter().position(|entry| {
                entry.as_ref().eq_ignore_ascii_case(name)
                    || entry.as_ref().eq_ignore_ascii_case(target.identifier())
            });
            let (is_pinned, order) = match pinned_order {
                Some(order) => (true, order as u32),
                None => (false, 0),
            };
            self.scored_buf.push((is_pinned, order, score, i));
        }
        self.scored_buf.sort_unstable_by(|a, b| {
            // Pinned items first; within a group keep the relevant order
            // (pinned: config order, non-pinned: score then original index).
            b.0.cmp(&a.0).then_with(|| {
                if a.0 && b.0 {
                    a.1.cmp(&b.1)
                } else {
                    b.2.cmp(&a.2).then(a.3.cmp(&b.3))
                }
            })
        });
        out_filtered.clear();
        out_filtered.extend(self.scored_buf.iter().map(|&(_, _, _, i)| i));
    }
}

// ---------------------------------------------------------------------------
// Match highlighting
// ---------------------------------------------------------------------------

/// Scratch state for computing match ranges. A `Matcher` owns a large
/// (~135 KB) heap working set, so it is created once per thread and reused.
struct RangeMatcher {
    matcher: Matcher,
    hay_buf: Vec<char>,
    needle_buf: Vec<char>,
    indices: Vec<u32>,
    lower_query: String,
    runs: Vec<(usize, usize)>,
    output: Vec<std::ops::Range<usize>>,
}

impl RangeMatcher {
    fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            hay_buf: Vec::new(),
            needle_buf: Vec::new(),
            indices: Vec::new(),
            lower_query: String::new(),
            runs: Vec::new(),
            output: Vec::new(),
        }
    }

    fn ranges(&mut self, name: &str, query: &str) -> &[std::ops::Range<usize>] {
        self.hay_buf.clear();
        self.needle_buf.clear();
        self.indices.clear();
        self.output.clear();
        self.lower_query.clear();
        self.lower_query
            .extend(query.chars().flat_map(|c| c.to_lowercase()));

        let hay = Utf32Str::new(name, &mut self.hay_buf);
        let needle = Utf32Str::new(&self.lower_query, &mut self.needle_buf);
        if self
            .matcher
            .fuzzy_indices(hay, needle, &mut self.indices)
            .is_none()
        {
            return &[];
        }

        // Merge consecutive char indices into runs, then map to byte offsets
        // (char-aligned, as required by GPUI highlight ranges).
        self.runs.clear();
        for &ix in &self.indices {
            let ix = ix as usize;
            match self.runs.last_mut() {
                Some((_, end)) if *end == ix => *end = ix + 1,
                _ => self.runs.push((ix, ix + 1)),
            }
        }
        for &(s, e) in &self.runs {
            let sb = byte_at(name, s).unwrap_or(name.len());
            let eb = byte_at(name, e).unwrap_or(name.len());
            let range = sb..eb;
            if !range.is_empty() {
                self.output.push(range);
            }
        }
        &self.output
    }
}

/// Byte offset of the `ix`-th char in `s`.
fn byte_at(s: &str, ix: usize) -> Option<usize> {
    s.char_indices().nth(ix).map(|(b, _)| b)
}

thread_local! {
    static RANGE_MATCHER: std::cell::RefCell<RangeMatcher> =
        std::cell::RefCell::new(RangeMatcher::new());
}

/// Byte ranges (char-aligned) of the query's matched characters inside
/// `name` — the same fuzzy, case-insensitive match as
/// [`SearchIndex::search`], so a row shown in the list always highlights.
/// Returns an empty vec when the query does not match the name.
pub fn highlight_ranges(name: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() || name.is_empty() {
        return Vec::new();
    }
    RANGE_MATCHER.with(|m| m.borrow_mut().ranges(name, query).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn highlight_ranges_empty_query() {
        assert!(highlight_ranges("Firefox", "").is_empty());
        assert!(highlight_ranges("Firefox", "   ").is_empty());
    }

    #[test]
    fn highlight_ranges_no_match() {
        assert!(highlight_ranges("Firefox", "zzz").is_empty());
    }

    #[test]
    fn highlight_ranges_fuzzy_match() {
        let name = "Firefox";
        let r = highlight_ranges(name, "fx");
        assert_eq!(r.len(), 2);
        let matched: String = r.iter().map(|x| name[x.clone()].to_string()).collect();
        // Matched chars keep their original case.
        assert_eq!(matched.to_lowercase(), "fx");
    }

    #[test]
    fn highlight_ranges_case_insensitive() {
        let name = "Firefox";
        let r = highlight_ranges(name, "FX");
        let matched: String = r.iter().map(|x| name[x.clone()].to_string()).collect();
        assert_eq!(matched.to_lowercase(), "fx");
    }

    #[test]
    fn highlight_ranges_consecutive_chars_merge() {
        let name = "Firefox";
        let r = highlight_ranges(name, "fi");
        assert_eq!(r.len(), 1);
        assert_eq!(name[r[0].clone()].to_lowercase(), "fi");
    }

    #[test]
    fn highlight_ranges_are_char_aligned() {
        // The haystack contains the two-byte é; byte offsets must stay on
        // char boundaries (GPUI assert).
        let name = "café app";
        let r = highlight_ranges(name, "fa");
        assert_eq!(r.len(), 2);
        let matched: String = r.iter().map(|x| name[x.clone()].to_string()).collect();
        assert_eq!(matched, "fa");
        for x in &r {
            assert!(name.is_char_boundary(x.start));
            assert!(name.is_char_boundary(x.end));
        }
    }

    #[test]
    fn highlight_ranges_trims_query() {
        let name = "Firefox";
        let padded = highlight_ranges(name, "  fx ");
        let plain = highlight_ranges(name, "fx");
        assert_eq!(padded, plain);
    }

    use crate::core::history::ExecutionRecord;

    fn aliases(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn target(name: &str) -> Target {
        Target::Script {
            name: name.into(),
            mode: crate::core::item::ScriptMode::FullOutput,
            icon: None,
            icon_image_path: None,
            path: std::sync::Arc::from(PathBuf::from(name)),
            metadata: std::sync::Arc::default(),
            metatags: crate::core::item::ScriptMetatags::default(),
            inline_output: None,
        }
    }

    /// A target whose display name differs from its on-disk path, so a test
    /// can pin it by path (identifier) rather than by name.
    fn target_with_path(name: &str, path: &str) -> Target {
        Target::Script {
            name: name.into(),
            mode: crate::core::item::ScriptMode::FullOutput,
            icon: None,
            icon_image_path: None,
            path: std::sync::Arc::from(PathBuf::from(path)),
            metadata: std::sync::Arc::default(),
            metatags: crate::core::item::ScriptMetatags::default(),
            inline_output: None,
        }
    }

    fn names(results: &[Target]) -> Vec<&str> {
        results.iter().map(Target::name).collect()
    }

    fn empty_history() -> History {
        History::test_new(PathBuf::new(), Vec::new())
    }

    fn search_helper(
        idx: &mut SearchIndex,
        history: &History,
        targets: &[Target],
        query: &str,
    ) -> Vec<Target> {
        let mut results = Vec::new();
        idx.search(query, targets, history, &mut results);
        results.into_iter().map(|i| targets[i].clone()).collect()
    }

    /// Uppercase letters in the query used to abort the process: nucleo
    /// lowercases the haystack (ignore_case) but compares needle bytes
    /// verbatim, so an uppercase needle char could pass the prefilter and
    /// still fail inside the optimal matcher ("should have been caught by
    /// prefilter" panic).
    #[test]
    fn uppercase_query_does_not_panic_and_matches() {
        let mut idx = SearchIndex::new(&HashMap::new(), &[]);
        let history = empty_history();
        let targets = [
            target("Safari"),
            target("Chrome"),
            target("Calculator"),
            target("Terminal"),
        ];
        for q in ["Sa", "Ch", "Ca", "Te", "SAFARI", "sA", "cH"] {
            let results = search_helper(&mut idx, &history, &targets, q);
            assert!(!results.is_empty(), "query {q:?} should match something");
        }
        let results = search_helper(&mut idx, &history, &targets, "Sa");
        assert_eq!(names(&results)[0], "Safari");
    }

    /// A launch recorded "just now" (100 frecency points).
    fn fresh_record(identifier: &str) -> ExecutionRecord {
        ExecutionRecord {
            target_identifier: identifier.into(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    #[test]
    fn matches_names_without_aliases() {
        let mut idx = SearchIndex::new(&HashMap::new(), &[]);
        let history = empty_history();
        let targets = [target("Git Status"), target("Grep")];
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "gr")),
            vec!["Grep"]
        );
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "")),
            vec!["Git Status", "Grep"]
        );
    }

    #[test]
    fn alias_matches_when_name_does_not() {
        let mut idx = SearchIndex::new(&aliases(&[("rm", "Uninstaller")]), &[]);
        let history = empty_history();
        let targets = [target("Uninstaller")];
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "rm")),
            vec!["Uninstaller"]
        );
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "inst")),
            vec!["Uninstaller"]
        );
        assert!(search_helper(&mut idx, &history, &targets, "zzz").is_empty());
    }

    #[test]
    fn name_still_matches_with_alias_configured() {
        let mut idx = SearchIndex::new(&aliases(&[("notes", "TextEdit")]), &[]);
        let history = empty_history();
        let targets = [target("TextEdit")];
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "edit")),
            vec!["TextEdit"]
        );
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "note")),
            vec!["TextEdit"]
        );
    }

    #[test]
    fn alias_only_match_ranks_first() {
        let mut idx = SearchIndex::new(&aliases(&[("un", "Unpack"), ("extract", "Unpack")]), &[]);
        let history = empty_history();
        let targets = [target("Unpack"), target("Grep")];
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "extract"))[0],
            "Unpack"
        );
    }

    #[test]
    fn alias_to_missing_target_never_matches() {
        let mut idx = SearchIndex::new(&aliases(&[("zz", "Ghost App")]), &[]);
        let history = empty_history();
        let targets = [target("Grep")];
        assert!(search_helper(&mut idx, &history, &targets, "zz").is_empty());
    }

    #[test]
    fn all_results_returned_without_cap() {
        let mut idx = SearchIndex::new(&HashMap::new(), &[]);
        let history = empty_history();
        let targets = [
            target("Alpha"),
            target("Bravo"),
            target("Charlie"),
            target("Delta"),
        ];
        let results = search_helper(&mut idx, &history, &targets, "");
        assert_eq!(results.len(), 4);
        let results = search_helper(&mut idx, &history, &targets, "a");
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn empty_query_sorts_by_frecency_desc() {
        let mut idx = SearchIndex::new(&HashMap::new(), &[]);
        let history = History::test_new(PathBuf::new(), vec![fresh_record("B")]);
        let targets = [target("A"), target("B"), target("C")];
        // B has a recent launch; A and C (frecency 0) keep their order.
        let results = search_helper(&mut idx, &history, &targets, "");
        assert_eq!(names(&results), vec!["B", "A", "C"]);
    }

    #[test]
    fn frecency_boosts_fuzzy_ranking() {
        let mut idx = SearchIndex::new(&HashMap::new(), &[]);
        // Five recent launches of "Zebra" (500 points) beat the stronger
        // fuzzy match of "Zed".
        let records = (0..5).map(|_| fresh_record("Zebra")).collect();
        let history = History::test_new(PathBuf::new(), records);
        let targets = [target("Zed"), target("Zebra")];
        let results = search_helper(&mut idx, &history, &targets, "z");
        assert_eq!(results[0].name(), "Zebra");
    }

    #[test]
    fn pinned_items_come_first_in_config_order() {
        // Pinned order is C then A (config order); B has recent frecency and
        // D is a plain zero-score item. Pinned items lead, then the rest.
        let history = History::test_new(PathBuf::new(), vec![fresh_record("B")]);
        let targets = [target("A"), target("B"), target("C"), target("D")];
        let mut idx = SearchIndex::new(&HashMap::new(), &["C".into(), "A".into()]);
        let results = search_helper(&mut idx, &history, &targets, "");
        assert_eq!(names(&results), vec!["C", "A", "B", "D"]);
    }

    #[test]
    fn pinned_item_outranks_fuzzy_and_frecency() {
        // "Zebra" has strong frecency and matches "a"; "Apple" also matches
        // "a" but is pinned, so it ranks first regardless.
        let records = (0..5).map(|_| fresh_record("Zebra")).collect();
        let history = History::test_new(PathBuf::new(), records);
        let targets = [target("Zebra"), target("Apple")];
        let mut idx = SearchIndex::new(&HashMap::new(), &["Apple".into()]);
        let results = search_helper(&mut idx, &history, &targets, "a");
        assert_eq!(names(&results), vec!["Apple", "Zebra"]);
    }

    #[test]
    fn pinned_item_hidden_when_query_does_not_match() {
        // "Pinned" does not fuzzy-match the query, so it is filtered out even
        // though it would otherwise be pinned to the top.
        let mut idx = SearchIndex::new(&HashMap::new(), &["Pinned".into()]);
        let history = empty_history();
        let targets = [target("Pinned"), target("Other")];
        let results = search_helper(&mut idx, &history, &targets, "oth");
        assert_eq!(names(&results), vec!["Other"]);
    }

    #[test]
    fn pinned_by_path_matches() {
        // The script's display name does not contain the pin string; only its
        // on-disk path (identifier) does, proving path-based pinning.
        let script = target_with_path("My Script", "/usr/local/bin/my-script.sh");
        let other = target("Other");
        let mut idx = SearchIndex::new(
            &HashMap::new(),
            &["/usr/local/bin/my-script.sh".to_string()],
        );
        let history = empty_history();
        let targets = [other, script];
        let results = search_helper(&mut idx, &history, &targets, "");
        assert_eq!(names(&results), vec!["My Script", "Other"]);
    }
}
