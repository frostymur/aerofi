//! nucleo-matcher wrapper: ranks display names against a filter query.

use nucleo_matcher::{Config, Matcher, Utf32Str};

/// A reusable fuzzy matcher. Holds the nucleo `Matcher` (it allocates a
/// working set up front, so it is built once and reused).
pub struct SearchIndex {
    matcher: Matcher,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// Rank `names` against `query`, best match first.
    ///
    /// Returns indices into `names` sorted by descending match score; ties
    /// keep the original order. An empty query scores 0 and matches
    /// everything (see nucleo docs).
    pub fn search(&mut self, names: &[&str], query: &str) -> Vec<usize> {
        // Separate scratch buffers so the needle and each haystack don't
        // fight over a single `Vec<char>` (non-ASCII only).
        let mut needle_buf: Vec<char> = Vec::new();
        let mut hay_buf: Vec<char> = Vec::new();
        let needle = Utf32Str::new(query, &mut needle_buf);

        let mut scored: Vec<(u16, usize)> = Vec::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            let hay = Utf32Str::new(name, &mut hay_buf);
            if let Some(score) = self.matcher.fuzzy_match(hay, needle) {
                scored.push((score, i));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}
