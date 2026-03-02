//! `FuzzyAhoCorasick` compatibility wrapper.

use super::matches::{FuzzyMatch, FuzzyMatches, Segment};
use super::pattern::Pattern;
use crate::FuzzyRegexBuilder;
use crate::api::regex::FuzzyRegex;
use crate::engine::MultiBitapMatcher;
use crate::engine::damlev::EditLimits;
use crate::types::{FuzzyLimits, FuzzyPenalties};
use std::borrow::Cow;

/// Escape special regex characters in a pattern string.
fn escape_pattern(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
            | '#' | '&' | '-' | '~' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

/// Pre-compiled pattern with its regex
struct CompiledPattern {
    pattern: Pattern,
    regex: FuzzyRegex,
}

/// Builder for [`FuzzyAhoCorasick`].
///
/// This provides a compatible API with the original fuzzy-aho-corasick builder.
///
/// # Example
///
/// ```rust
/// use fuzzy_regex::compat::fac::FuzzyAhoCorasickBuilder;
/// use fuzzy_regex::types::FuzzyLimits;
///
/// let engine = FuzzyAhoCorasickBuilder::new()
///     .fuzzy(FuzzyLimits::new().edits(1))
///     .case_insensitive(true)
///     .build(["hello", "world"]);
///
/// let matches = engine.search("helo wrld", 0.8);
/// assert!(!matches.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct FuzzyAhoCorasickBuilder {
    limits: Option<FuzzyLimits>,
    penalties: FuzzyPenalties,
    case_insensitive: bool,
}

impl FuzzyAhoCorasickBuilder {
    /// Create a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set global fuzzy limits for all patterns.
    #[must_use]
    pub fn fuzzy(mut self, limits: FuzzyLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Set custom penalty weights.
    #[must_use]
    pub fn penalties(mut self, penalties: FuzzyPenalties) -> Self {
        self.penalties = penalties;
        self
    }

    /// Enable case-insensitive matching.
    #[must_use]
    pub fn case_insensitive(mut self, value: bool) -> Self {
        self.case_insensitive = value;
        self
    }

    /// Build the automaton from patterns.
    pub fn build<T>(self, inputs: impl IntoIterator<Item = T>) -> FuzzyAhoCorasick
    where
        T: Into<Pattern>,
    {
        let patterns: Vec<Pattern> = inputs.into_iter().map(Into::into).collect();

        // Pre-compile all regexes at build time
        let patterns_vec: Vec<Pattern> = patterns.into_iter().collect();

        let compiled: Vec<CompiledPattern> = patterns_vec
            .iter()
            .filter_map(|pattern| {
                // Determine edit limit for this pattern
                let edits = pattern
                    .limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_edits)
                    .or_else(|| self.limits.as_ref().and_then(FuzzyLimits::get_edits))
                    .unwrap_or(0);

                // Build regex pattern - escape special chars
                let escaped = escape_pattern(&pattern.pattern);
                let regex_pattern = format!("(?:{escaped})~{edits}");

                // Build limits for this pattern
                let limits = pattern
                    .limits
                    .clone()
                    .or_else(|| self.limits.clone())
                    .unwrap_or_default();

                let regex = FuzzyRegexBuilder::new(&regex_pattern)
                    .case_insensitive(self.case_insensitive)
                    .fuzzy(limits)
                    .build()
                    .ok()?;

                Some(CompiledPattern {
                    pattern: pattern.clone(),
                    regex,
                })
            })
            .collect();

        // Try to create a MultiBitapMatcher for efficient multi-pattern search
        // This processes all patterns in a single text pass
        let multi_bitap = if compiled.len() > 1 {
            // Check if all patterns have the same edit limit and are short enough for Bitap
            let first_edits = compiled
                .first()
                .and_then(|c| c.pattern.limits.as_ref())
                .and_then(FuzzyLimits::get_edits)
                .or_else(|| self.limits.as_ref().and_then(FuzzyLimits::get_edits))
                .unwrap_or(0);

            let all_same_edits = compiled.iter().all(|c| {
                let edits = c
                    .pattern
                    .limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_edits)
                    .or_else(|| self.limits.as_ref().and_then(FuzzyLimits::get_edits))
                    .unwrap_or(0);
                edits == first_edits
            });

            // Check all patterns are ≤64 chars (Bitap limit)
            let all_short = compiled
                .iter()
                .all(|c| c.pattern.pattern.chars().count() <= 64);

            if all_same_edits && all_short {
                let patterns: Vec<&str> = compiled
                    .iter()
                    .map(|c| c.pattern.pattern.as_str())
                    .collect();
                let limits = EditLimits::new(first_edits);
                MultiBitapMatcher::new(&patterns, &limits, self.case_insensitive)
            } else {
                None
            }
        } else {
            None
        };

        FuzzyAhoCorasick {
            compiled,
            penalties: self.penalties,
            multi_bitap,
        }
    }
}

/// Fuzzy Aho-Corasick compatible search engine.
///
/// This wraps the fuzzy-regex engine to provide a compatible API with
/// the original fuzzy-aho-corasick library.
pub struct FuzzyAhoCorasick {
    /// Pre-compiled patterns with their regexes.
    compiled: Vec<CompiledPattern>,
    /// Penalty weights for edit operations.
    penalties: FuzzyPenalties,
    /// Optional multi-pattern Bitap matcher for efficient search.
    multi_bitap: Option<MultiBitapMatcher>,
}

impl std::fmt::Debug for FuzzyAhoCorasick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let patterns: Vec<&Pattern> = self.compiled.iter().map(|c| &c.pattern).collect();
        f.debug_struct("FuzzyAhoCorasick")
            .field("patterns", &patterns)
            .field("multi_bitap_active", &self.multi_bitap.is_some())
            .finish_non_exhaustive()
    }
}

impl FuzzyAhoCorasick {
    /// Get the patterns this automaton was built with.
    #[must_use]
    pub fn patterns(&self) -> Vec<&Pattern> {
        self.compiled.iter().map(|c| &c.pattern).collect()
    }

    /// Get the penalty weights configured for this automaton.
    #[must_use]
    pub fn penalties(&self) -> &FuzzyPenalties {
        &self.penalties
    }

    /// Search for all fuzzy matches above the similarity threshold.
    ///
    /// Returns matches sorted by default ranking (similarity desc, pattern length desc,
    /// text length desc, start asc).
    #[must_use]
    pub fn search<'a>(&'a self, haystack: &'a str, similarity_threshold: f32) -> FuzzyMatches<'a> {
        let mut matches = self.search_unsorted(haystack, similarity_threshold);
        matches.default_sort();
        matches
    }

    /// Search without sorting the results.
    #[must_use]
    pub fn search_unsorted<'a>(
        &'a self,
        haystack: &'a str,
        similarity_threshold: f32,
    ) -> FuzzyMatches<'a> {
        use crate::engine::FxHashMap;
        let mut best: FxHashMap<(usize, usize, usize), FuzzyMatch<'a>> = FxHashMap::default();

        // Fast path: use MultiBitapMatcher for single-pass multi-pattern search
        if let Some(ref multi) = self.multi_bitap {
            let all_matches = multi.find_all(haystack, similarity_threshold);

            for m in all_matches {
                let pattern_index = m.pattern_index;
                let compiled = &self.compiled[pattern_index];
                let result = &m.match_result;

                let key = (result.start, result.end, pattern_index);

                let fm = FuzzyMatch {
                    insertions: result.insertions,
                    deletions: result.deletions,
                    substitutions: result.substitutions,
                    swaps: result.swaps,
                    edits: result.total_edits(),
                    pattern_index,
                    pattern: &compiled.pattern,
                    start: result.start,
                    end: result.end,
                    similarity: result.similarity,
                    text: &haystack[result.start..result.end],
                };

                best.entry(key)
                    .and_modify(|existing| {
                        if fm.similarity > existing.similarity {
                            *existing = fm.clone();
                        }
                    })
                    .or_insert(fm);
            }
        } else {
            // Fallback: search each pattern individually
            for (pattern_index, compiled) in self.compiled.iter().enumerate() {
                for m in compiled.regex.find_all_overlapping(haystack) {
                    if m.similarity() < similarity_threshold {
                        continue;
                    }

                    let key = (m.start(), m.end(), pattern_index);
                    let edits = m.edits();

                    let text = m.as_str().to_string();
                    let fm = FuzzyMatch {
                        insertions: edits.insertions,
                        deletions: edits.deletions,
                        substitutions: edits.substitutions,
                        swaps: 0,
                        edits: edits.total(),
                        pattern_index,
                        pattern: &compiled.pattern,
                        start: m.start(),
                        end: m.end(),
                        similarity: m.similarity(),
                        text: Box::leak(text.into_boxed_str()),
                    };

                    best.entry(key)
                        .and_modify(|existing| {
                            if fm.similarity > existing.similarity {
                                *existing = fm.clone();
                            }
                        })
                        .or_insert(fm);
                }
            }
        }

        FuzzyMatches {
            haystack,
            inner: best.into_values().collect(),
        }
    }

    /// Search with greedy sorting (pattern length first).
    #[must_use]
    pub fn search_greedy<'a>(
        &'a self,
        haystack: &'a str,
        similarity_threshold: f32,
    ) -> FuzzyMatches<'a> {
        let mut matches = self.search_unsorted(haystack, similarity_threshold);
        matches.greedy_sort();
        matches
    }

    /// Search with coverage-weighted sorting.
    #[must_use]
    pub fn search_coverage_weighted<'a>(
        &'a self,
        haystack: &'a str,
        similarity_threshold: f32,
    ) -> FuzzyMatches<'a> {
        let mut matches = self.search_unsorted(haystack, similarity_threshold);
        matches.coverage_weighted_sort();
        matches
    }

    /// Search returning only non-overlapping matches.
    #[must_use]
    pub fn search_non_overlapping<'a>(
        &'a self,
        haystack: &'a str,
        similarity_threshold: f32,
    ) -> FuzzyMatches<'a> {
        // Fast path: single pattern - use filtered search, skip post-filtering
        if self.compiled.len() == 1 {
            let compiled = &self.compiled[0];
            // Use filtered version to avoid creating Match objects we'll discard
            let all_matches = compiled
                .regex
                .find_all_overlapping_filtered(haystack, similarity_threshold);

            // Convert to FuzzyMatch
            let mut matches: Vec<FuzzyMatch<'a>> = all_matches
                .into_iter()
                .map(|m| {
                    let edits = m.edits();
                    let text = m.as_str().to_string();
                    FuzzyMatch {
                        insertions: edits.insertions,
                        deletions: edits.deletions,
                        substitutions: edits.substitutions,
                        swaps: 0,
                        edits: edits.total(),
                        pattern_index: 0,
                        pattern: &compiled.pattern,
                        start: m.start(),
                        end: m.end(),
                        similarity: m.similarity(),
                        text: Box::leak(text.into_boxed_str()),
                    }
                })
                .collect();

            // Sort by similarity desc, then by start position
            matches.sort_by(|a, b| {
                b.similarity
                    .total_cmp(&a.similarity)
                    .then_with(|| a.start.cmp(&b.start))
            });

            // Pick non-overlapping greedily (best similarity first)
            let mut result = Vec::new();
            let mut last_end = 0;
            for m in matches {
                if m.start >= last_end {
                    last_end = m.end;
                    result.push(m);
                }
            }
            result.sort_by_key(|m| m.start);

            return FuzzyMatches {
                haystack,
                inner: result,
            };
        }

        // Multi-pattern: use full search with deduplication
        let mut matches = self.search(haystack, similarity_threshold);
        matches.non_overlapping();
        matches
    }

    /// Search returning non-overlapping unique matches.
    #[must_use]
    pub fn search_non_overlapping_unique<'a>(
        &'a self,
        haystack: &'a str,
        similarity_threshold: f32,
    ) -> FuzzyMatches<'a> {
        let mut matches = self.search(haystack, similarity_threshold);
        matches.non_overlapping_unique();
        matches
    }

    /// Search with coverage-weighted sorting and unique non-overlapping.
    #[must_use]
    pub fn search_non_overlapping_unique_coverage_weighted<'a>(
        &'a self,
        haystack: &'a str,
        similarity_threshold: f32,
    ) -> FuzzyMatches<'a> {
        let mut matches = self.search_coverage_weighted(haystack, similarity_threshold);
        matches.non_overlapping_unique();
        matches
    }

    /// Replace matches using a callback.
    #[must_use]
    pub fn replace<'a, F, S>(&'a self, text: &'a str, callback: F, threshold: f32) -> String
    where
        F: Fn(&FuzzyMatch<'a>) -> Option<S>,
        S: Into<Cow<'a, str>>,
    {
        self.search_non_overlapping(text, threshold)
            .replace(callback)
    }

    /// Strip matched prefix.
    #[must_use]
    pub fn strip_prefix<'a>(&'a self, haystack: &'a str, threshold: f32) -> String {
        self.search_non_overlapping(haystack, threshold)
            .strip_prefix()
    }

    /// Strip matched suffix.
    #[must_use]
    pub fn strip_postfix<'a>(&'a self, haystack: &'a str, threshold: f32) -> String {
        self.search_non_overlapping(haystack, threshold)
            .strip_postfix()
    }

    /// Split text by matches.
    pub fn split<'a>(
        &'a self,
        haystack: &'a str,
        threshold: f32,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.search_non_overlapping(haystack, threshold).split()
    }

    /// Iterate over segments (matched and unmatched).
    pub fn segment_iter<'a>(
        &'a self,
        haystack: &'a str,
        threshold: f32,
    ) -> impl Iterator<Item = Segment<'a>> {
        self.search_non_overlapping(haystack, threshold)
            .segment_iter()
    }

    /// Get segmented text with smart spacing.
    #[must_use]
    pub fn segment_text(&self, haystack: &str, threshold: f32) -> String {
        self.search_non_overlapping(haystack, threshold)
            .segment_text()
    }
}
