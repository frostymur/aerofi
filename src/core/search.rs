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
    needle_buf: Vec<char>,
    hay_buf: Vec<char>,
    /// Reused scratch buffer for (score, index) pairs — avoids a per-keystroke heap allocation.
    scored_buf: Vec<(u32, usize)>,
}

impl SearchIndex {
    /// Build an index that also matches the given `aliases` (alias ->
    /// target display name). Alias values must equal the target's display
    /// name exactly; aliases pointing to a target that is not in the
    /// searched list simply never match.
    pub fn new(aliases: &HashMap<String, String>) -> Self {
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
            needle_buf: Vec::new(),
            hay_buf: Vec::new(),
            scored_buf: Vec::new(),
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
        let mut lower_query = String::new();
        lower_query.extend(query.chars().flat_map(|c| c.to_lowercase()));
        let query = lower_query.as_str();

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
            self.scored_buf.push((u32::from(fuzzy_score) + frecency, i));
        }
        self.scored_buf
            .sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        out_filtered.clear();
        out_filtered.extend(self.scored_buf.iter().map(|&(_, i)| i));
    }

    /// Rank `targets` against `query`, boosted by the frecency scores
    /// from `history`, and return all matching ones as a ready-to-render
    /// `Vec<Target>`, best match first.
    #[allow(dead_code)]
    pub fn filter_and_rank(
        &mut self,
        history: &History,
        targets: &[Target],
        query: &str,
    ) -> Vec<Target> {
        let mut indices = Vec::new();
        self.search(query, targets, history, &mut indices);
        indices.iter().map(|&i| targets[i].clone()).collect()
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
}

impl RangeMatcher {
    fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            hay_buf: Vec::new(),
            needle_buf: Vec::new(),
            indices: Vec::new(),
            lower_query: String::new(),
        }
    }

    fn ranges(&mut self, name: &str, query: &str) -> Vec<std::ops::Range<usize>> {
        self.hay_buf.clear();
        self.needle_buf.clear();
        self.indices.clear();
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
            return Vec::new();
        }

        // Merge consecutive char indices into runs, then map to byte offsets
        // (char-aligned, as required by GPUI highlight ranges).
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for &ix in &self.indices {
            let ix = ix as usize;
            match runs.last_mut() {
                Some((_, end)) if *end == ix => *end = ix + 1,
                _ => runs.push((ix, ix + 1)),
            }
        }
        runs.into_iter()
            .map(|(s, e)| {
                let sb = byte_at(name, s).unwrap_or(name.len());
                let eb = byte_at(name, e).unwrap_or(name.len());
                sb..eb
            })
            .filter(|r| !r.is_empty())
            .collect()
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
    RANGE_MATCHER.with(|m| m.borrow_mut().ranges(name, query))
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
            path: std::sync::Arc::from(PathBuf::from(name)),
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
        let mut idx = SearchIndex::new(&HashMap::new());
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
        let mut idx = SearchIndex::new(&HashMap::new());
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
        let mut idx = SearchIndex::new(&aliases(&[("rm", "Uninstaller")]));
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
        let mut idx = SearchIndex::new(&aliases(&[("notes", "TextEdit")]));
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
        let mut idx = SearchIndex::new(&aliases(&[("un", "Unpack"), ("extract", "Unpack")]));
        let history = empty_history();
        let targets = [target("Unpack"), target("Grep")];
        assert_eq!(
            names(&search_helper(&mut idx, &history, &targets, "extract"))[0],
            "Unpack"
        );
    }

    #[test]
    fn alias_to_missing_target_never_matches() {
        let mut idx = SearchIndex::new(&aliases(&[("zz", "Ghost App")]));
        let history = empty_history();
        let targets = [target("Grep")];
        assert!(search_helper(&mut idx, &history, &targets, "zz").is_empty());
    }

    #[test]
    fn all_results_returned_without_cap() {
        let mut idx = SearchIndex::new(&HashMap::new());
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
        let mut idx = SearchIndex::new(&HashMap::new());
        let history = History::test_new(PathBuf::new(), vec![fresh_record("B")]);
        let targets = [target("A"), target("B"), target("C")];
        // B has a recent launch; A and C (frecency 0) keep their order.
        let results = search_helper(&mut idx, &history, &targets, "");
        assert_eq!(names(&results), vec!["B", "A", "C"]);
    }

    #[test]
    fn frecency_boosts_fuzzy_ranking() {
        let mut idx = SearchIndex::new(&HashMap::new());
        // Five recent launches of "Zebra" (500 points) beat the stronger
        // fuzzy match of "Zed".
        let records = (0..5).map(|_| fresh_record("Zebra")).collect();
        let history = History::test_new(PathBuf::new(), records);
        let targets = [target("Zed"), target("Zebra")];
        let results = search_helper(&mut idx, &history, &targets, "z");
        assert_eq!(results[0].name(), "Zebra");
    }

    #[test]
    fn test_search_memory_growth() {
        use std::process::Command;
        fn get_rss() -> usize {
            let pid = std::process::id();
            let output = Command::new("ps")
                .args(["-o", "rss=", "-p", &pid.to_string()])
                .output()
                .ok();
            if let Some(out) = output {
                let s = String::from_utf8_lossy(&out.stdout);
                s.trim().parse::<usize>().unwrap_or(0)
            } else {
                0
            }
        }

        let mut idx = SearchIndex::new(&HashMap::new());
        let history = empty_history();
        // Generate 100 targets
        let targets: Vec<Target> = (0..100)
            .map(|i| target(&format!("app-name-{}", i)))
            .collect();

        let initial_rss = get_rss();
        println!("Initial RSS (benchmark): {} KB", initial_rss);

        let queries = ["a", "g", "s", "c", "app", "name", "99", "1", "", "foo"];
        for i in 0..100000 {
            let q = queries[i % queries.len()];
            let _results = search_helper(&mut idx, &history, &targets, q);
        }

        let final_rss = get_rss();
        println!("Final RSS after 100,000 runs: {} KB", final_rss);
        let delta = final_rss.saturating_sub(initial_rss);
        println!("Delta RSS: {} KB", delta);

        // Memory should not leak / should not grow by more than a reasonable threshold (e.g. 4096KB)
        // because of SharedString reference counting.
        assert!(delta < 4096, "Memory delta was too large: {} KB", delta);
    }
}
