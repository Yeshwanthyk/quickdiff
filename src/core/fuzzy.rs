//! Fuzzy matching for file path filtering.

use nucleo_matcher::{
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32Str,
};

/// Fuzzy matcher wrapping nucleo-matcher.
///
/// Reuses internal buffers across calls for efficiency.
pub struct FuzzyMatcher {
    matcher: Matcher,
    buf: Vec<char>,
}

impl Default for FuzzyMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyMatcher {
    /// Create a new fuzzy matcher with sensible defaults for path matching.
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            buf: Vec::with_capacity(256),
        }
    }

    /// Filter and sort candidates by match score.
    ///
    /// Returns indices of matching candidates, sorted by score (highest first).
    pub fn filter_sorted<I, S>(&mut self, pattern: &str, candidates: I) -> Vec<usize>
    where
        I: Iterator<Item = (usize, S)>,
        S: AsRef<str>,
    {
        if pattern.is_empty() {
            return Vec::new();
        }

        let pat = Pattern::new(
            pattern,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        let mut results: Vec<(usize, u32)> = candidates
            .filter_map(|(idx, s)| {
                self.buf.clear();
                let haystack = Utf32Str::new(s.as_ref(), &mut self.buf);
                pat.score(haystack, &mut self.matcher).map(|sc| (idx, sc))
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.1.cmp(&a.1));

        results.into_iter().map(|(i, _)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pattern_returns_empty() {
        let mut m = FuzzyMatcher::new();
        let results = m.filter_sorted("", ["a", "b"].into_iter().enumerate());
        assert!(results.is_empty());
    }

    #[test]
    fn test_filter_sorted_basic() {
        let mut m = FuzzyMatcher::new();
        let candidates = ["main.rs", "app.rs", "lib.rs", "fuzzy.rs"];
        let results = m.filter_sorted("rs", candidates.iter().enumerate().map(|(i, s)| (i, *s)));
        // All should match since they all contain "rs"
        assert_eq!(results.len(), 4);
        // Results should be valid indices
        for idx in &results {
            assert!(*idx < candidates.len());
        }
    }

    #[test]
    fn test_filter_sorted_no_match() {
        let mut m = FuzzyMatcher::new();
        let candidates = ["main.rs", "app.rs"];
        let results = m.filter_sorted("xyz", candidates.iter().enumerate().map(|(i, s)| (i, *s)));
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_match() {
        let mut m = FuzzyMatcher::new();
        let candidates = ["application.rs", "main.rs"];
        // "aplrs" should match "application.rs" fuzzily
        let results = m.filter_sorted("aplrs", candidates.iter().enumerate().map(|(i, s)| (i, *s)));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 0);
    }

    #[test]
    fn test_score_ordering() {
        let mut m = FuzzyMatcher::new();
        // Exact prefix should score higher than fuzzy match
        let candidates = ["something_app.rs", "app.rs", "zapp.rs"];
        let results = m.filter_sorted("app", candidates.iter().enumerate().map(|(i, s)| (i, *s)));
        // "app.rs" should be first (best match)
        assert!(!results.is_empty());
        assert_eq!(results[0], 1); // app.rs
    }
}
