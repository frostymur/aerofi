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
    pub fn filter_and_rank(
        &mut self,
        history: &History,
        targets: &[Target],
        query: &str,
        out_filtered: &mut Vec<usize>,
    ) {
        // Reuse all scratch buffers to avoid heap allocations every keystroke.
        self.needle_buf.clear();
        self.hay_buf.clear();
        self.scored_buf.clear();
        out_filtered.clear();
        let needle = Utf32Str::new(query, &mut self.needle_buf);
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
            let frecency = history.calculate_frecency(&target.identifier());
            self.scored_buf.push((u32::from(fuzzy_score) + frecency, i));
        }
        self.scored_buf
            .sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        out_filtered.extend(self.scored_buf.iter().map(|&(_, i)| i));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        idx.filter_and_rank(history, targets, query, &mut results);
        results.into_iter().map(|i| targets[i].clone()).collect()
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

        // Memory should not leak / should not grow by more than a reasonable threshold (e.g. 1000KB)
        // because of SharedString reference counting.
        assert!(delta < 1000, "Memory delta was too large: {} KB", delta);
    }
}
