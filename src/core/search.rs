//! nucleo-matcher wrapper: ranks targets (by display name or configured
//! aliases) against a filter query.

use std::collections::HashMap;

use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::core::item::Target;

/// A reusable fuzzy matcher. Holds the nucleo `Matcher` (it allocates a
/// working set up front, so it is built once and reused), a reverse alias
/// index (target display name -> the aliases that point to it), and the
/// UTF-32 scratch buffers. Everything is allocated once in `new` and
/// reused across `search()` calls.
pub struct SearchIndex {
    matcher: Matcher,
    aliases_by_target: HashMap<String, Vec<String>>,
    max_results: usize,
    needle_buf: Vec<char>,
    hay_buf: Vec<char>,
}

impl SearchIndex {
    /// Build an index that also matches the given `aliases` (alias ->
    /// target display name) and returns at most `max_results` items per
    /// query. Alias values must equal the target's display name exactly;
    /// aliases pointing to a target that is not in the searched list
    /// simply never match.
    pub fn new(aliases: &HashMap<String, String>, max_results: usize) -> Self {
        let mut aliases_by_target: HashMap<String, Vec<String>> = HashMap::new();
        for (alias, target) in aliases {
            aliases_by_target
                .entry(target.clone())
                .or_default()
                .push(alias.clone());
        }
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            aliases_by_target,
            max_results,
            needle_buf: Vec::new(),
            hay_buf: Vec::new(),
        }
    }

    /// Rank `targets` against `query` and return the best-matching ones
    /// (up to `max_results`) as a ready-to-render `Vec<Target>`, best
    /// match first.
    ///
    /// A target matches when either its display name or one of its aliases
    /// fuzzy-matches the query; the best of those scores is used for
    /// ranking; ties keep the original order. An empty query scores 0 and
    /// matches everything (see nucleo docs).
    pub fn search(&mut self, targets: &[Target], query: &str) -> Vec<Target> {
        self.needle_buf.clear();
        self.hay_buf.clear();
        let needle = Utf32Str::new(query, &mut self.needle_buf);

        let mut scored: Vec<(u16, usize)> = Vec::with_capacity(targets.len().min(self.max_results));
        for (i, target) in targets.iter().enumerate() {
            // Disjoint field borrows: `aliases_by_target` (shared),
            // `hay_buf` and `matcher` (`fuzzy_match` takes `&mut self`).
            let name = target.name();
            let aliases = self.aliases_by_target.get(name);
            let mut hay = Utf32Str::new(name, &mut self.hay_buf);
            let mut best = self.matcher.fuzzy_match(hay, needle);
            if let Some(list) = aliases {
                for alias in list {
                    hay = Utf32Str::new(alias, &mut self.hay_buf);
                    if let Some(score) = self.matcher.fuzzy_match(hay, needle) {
                        best = Some(best.map_or(score, |b| b.max(score)));
                    }
                }
            }
            if let Some(score) = best {
                scored.push((score, i));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(self.max_results)
            .map(|(_, i)| targets[i].clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn aliases(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn target(name: &str) -> Target {
        Target::Script {
            name: name.to_string(),
            mode: crate::core::item::ScriptMode::FullOutput,
            icon: None,
            path: PathBuf::from(name),
        }
    }

    fn names(results: &[Target]) -> Vec<&str> {
        results.iter().map(Target::name).collect()
    }

    #[test]
    fn matches_names_without_aliases() {
        let mut idx = SearchIndex::new(&HashMap::new(), 20);
        let targets = [target("Git Status"), target("Grep")];
        assert_eq!(names(&idx.search(&targets, "gr")), vec!["Grep"]);
        assert_eq!(names(&idx.search(&targets, "")), vec!["Git Status", "Grep"]);
    }

    #[test]
    fn alias_matches_when_name_does_not() {
        let mut idx = SearchIndex::new(&aliases(&[("rm", "Uninstaller")]), 20);
        let targets = [target("Uninstaller")];
        assert_eq!(names(&idx.search(&targets, "rm")), vec!["Uninstaller"]);
        assert_eq!(names(&idx.search(&targets, "inst")), vec!["Uninstaller"]);
        assert!(idx.search(&targets, "zzz").is_empty());
    }

    #[test]
    fn name_still_matches_with_alias_configured() {
        let mut idx = SearchIndex::new(&aliases(&[("notes", "TextEdit")]), 20);
        let targets = [target("TextEdit")];
        assert_eq!(names(&idx.search(&targets, "edit")), vec!["TextEdit"]);
        assert_eq!(names(&idx.search(&targets, "note")), vec!["TextEdit"]);
    }

    #[test]
    fn alias_only_match_ranks_first() {
        let mut idx = SearchIndex::new(&aliases(&[("un", "Unpack"), ("extract", "Unpack")]), 20);
        let targets = [target("Unpack"), target("Grep")];
        assert_eq!(names(&idx.search(&targets, "extract"))[0], "Unpack");
    }

    #[test]
    fn alias_to_missing_target_never_matches() {
        let mut idx = SearchIndex::new(&aliases(&[("zz", "Ghost App")]), 20);
        let targets = [target("Grep")];
        assert!(idx.search(&targets, "zz").is_empty());
    }

    #[test]
    fn results_capped_at_max_results() {
        let mut idx = SearchIndex::new(&HashMap::new(), 2);
        let targets = [
            target("Alpha"),
            target("Bravo"),
            target("Charlie"),
            target("Delta"),
        ];
        let results = idx.search(&targets, "");
        assert_eq!(names(&results), vec!["Alpha", "Bravo"]);
        let results = idx.search(&targets, "a");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name(), "Alpha");
    }
}
