//! Bridge for fuzzy literal matching using Levenshtein automata and Bitap.

#![allow(
    clippy::needless_range_loop,
    clippy::match_same_arms,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::float_cmp,
    clippy::allow_attributes
)]

use crate::types::{FuzzyLimits, FuzzyPenalties};
use std::cell::RefCell;

use super::GuardNfa;
use super::bitap::BitapMatcher;
use super::damlev::{DamLevNfa, EditLimits, SearchBuffers};
use super::hash::{FxHashMap, FxHashSet};
use crate::ir::{EditCharRestriction, LiteralPattern};

/// Cached search results from fuzzy search.
/// Maps (`pattern_index`, `start_position`) -> `Vec<FuzzyMatchResult>`
#[derive(Debug, Default)]
pub struct CachedMatches {
    /// Matches organized by (`pattern_index`, `start_byte_position`)
    by_pattern_and_start: FxHashMap<(usize, usize), Vec<FuzzyMatchResult>>,
}

impl CachedMatches {
    /// Get the best match for a pattern at a specific start position.
    #[must_use]
    pub fn get(&self, pattern_index: usize, start: usize) -> Option<&FuzzyMatchResult> {
        self.by_pattern_and_start
            .get(&(pattern_index, start))
            .and_then(|v| v.first())
    }

    /// Get all matches for a pattern at a specific start position.
    pub fn get_all(&self, pattern_index: usize, start: usize) -> Option<&[FuzzyMatchResult]> {
        self.by_pattern_and_start
            .get(&(pattern_index, start))
            .map(Vec::as_slice)
    }

    /// Insert a match result for a pattern at a specific start position.
    pub fn insert(&mut self, pattern_index: usize, start: usize, result: FuzzyMatchResult) {
        self.by_pattern_and_start
            .entry((pattern_index, start))
            .or_default()
            .push(result);
    }

    /// Iterate over all matches.
    pub fn iter(&self) -> impl Iterator<Item = ((usize, usize), &[FuzzyMatchResult])> + '_ {
        self.by_pattern_and_start
            .iter()
            .map(|((pattern_idx, start), results)| ((*pattern_idx, *start), results.as_slice()))
    }

    /// Check if the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_pattern_and_start.is_empty()
    }

    /// Shift every match position by `offset`: start keys and result ends move
    /// forward by `offset`. Used to reuse a search over a suffix `text[offset..]`
    /// as if it had run on the full text (the end-anchor windowed search).
    #[must_use]
    pub fn shifted(self, offset: usize) -> Self {
        if offset == 0 {
            return self;
        }
        let mut out: FxHashMap<(usize, usize), Vec<FuzzyMatchResult>> = FxHashMap::default();
        for ((pattern, start), mut results) in self.by_pattern_and_start {
            for r in &mut results {
                r.end += offset;
            }
            out.insert((pattern, start + offset), results);
        }
        CachedMatches {
            by_pattern_and_start: out,
        }
    }
}

/// Bridge to Levenshtein automata for efficient fuzzy literal matching.
#[derive(Debug)]
pub struct FuzzyBridge {
    /// One Levenshtein NFA per pattern.
    automata: Vec<DamLevNfa>,
    /// Guard-based NFA for fast first-match (single pattern only).
    #[allow(dead_code)]
    guard_nfa: Option<GuardNfa>,
    /// Bitap matchers for short patterns (≤64 chars). None if pattern is too long.
    bitap_matchers: Vec<Option<BitapMatcher>>,
    /// Pattern texts for reference.
    patterns: Vec<String>,
    /// Edit limits per pattern (for calculating effective thresholds).
    limits: Vec<Option<FuzzyLimits>>,
    /// Character class restrictions for edits (parallel to patterns).
    edit_char_restrictions: Vec<Option<EditCharRestriction>>,
    /// Whether any pattern has character restrictions (for fast path).
    has_char_restrictions: bool,
    /// Case insensitive mode.
    case_insensitive: bool,
    /// Reusable search buffers to avoid per-call allocations.
    search_buffers: RefCell<SearchBuffers>,
    /// Reusable text_chars buffer for Bitap to avoid Vec<char> allocation per call.
    text_chars_buf: RefCell<Vec<(usize, char)>>,
    /// Word list patterns (from \L<name>) added at runtime.
    word_list_patterns: Vec<String>,
    /// Word list edit limits.
    word_list_limits: Vec<Option<FuzzyLimits>>,
}

impl FuzzyBridge {
    /// Create a new fuzzy bridge from literal patterns.
    #[must_use]
    pub fn new(
        literals: &[LiteralPattern],
        _default_limits: Option<FuzzyLimits>,
        _penalties: Option<FuzzyPenalties>,
        case_insensitive: bool,
    ) -> Option<Self> {
        if literals.is_empty() {
            return None;
        }

        let patterns: Vec<String> = literals.iter().map(|l| l.text.clone()).collect();
        let limits: Vec<Option<FuzzyLimits>> = literals.iter().map(|l| l.limits.clone()).collect();
        let edit_char_restrictions: Vec<Option<EditCharRestriction>> =
            literals.iter().map(|l| l.edit_chars.clone()).collect();

        // Build Levenshtein NFA and Bitap matcher for each pattern
        let mut automata: Vec<DamLevNfa> = Vec::with_capacity(literals.len());
        let mut bitap_matchers: Vec<Option<BitapMatcher>> = Vec::with_capacity(literals.len());

        for lit in literals {
            let edit_limits = if let Some(ref lim) = lit.limits {
                // When e<= not specified, max_edits is the sum of individual limits
                let max_edits = lim.get_edits().unwrap_or_else(|| {
                    let i = lim.get_insertions().unwrap_or(0);
                    let d = lim.get_deletions().unwrap_or(0);
                    let s = lim.get_substitutions().unwrap_or(0);
                    let t = lim.get_swaps().unwrap_or(0);
                    i.saturating_add(d).saturating_add(s).saturating_add(t)
                });
                EditLimits::with_limits(
                    max_edits,
                    lim.get_insertions(),
                    lim.get_deletions(),
                    lim.get_substitutions(),
                    lim.get_swaps(),
                )
            } else {
                EditLimits::new(0) // Exact match
            };

            // Build Levenshtein NFA (always works)
            automata.push(DamLevNfa::new(
                &lit.text,
                edit_limits.clone(),
                case_insensitive,
            ));

            // Bitap only enforces the *total* edit budget; it cannot attribute
            // edits to operation types. If any per-operation cap is binding
            // (Some(x) with x < max_edits) Bitap would over-match, so skip it and
            // let every search path fall back to the Damerau-Levenshtein NFA,
            // which tracks per-operation counts. Exception: when the pattern has
            // an edit-character restriction, the matcher validates matches via
            // the Bitap path (`validate_edit_chars`), which the NFA path does not
            // do -- keep Bitap there.
            let max_edits = edit_limits.max_edits as usize;
            let has_binding_per_op = [
                edit_limits.max_insertions,
                edit_limits.max_deletions,
                edit_limits.max_substitutions,
                edit_limits.max_swaps,
            ]
            .iter()
            .any(|cap| matches!(cap, Some(x) if (*x as usize) < max_edits));
            let bitap = if has_binding_per_op && lit.edit_chars.is_none() {
                None
            } else {
                BitapMatcher::new(&lit.text, edit_limits, case_insensitive)
            };
            bitap_matchers.push(bitap);
        }

        // Build Guard NFA for single pattern (fast path for find_first)
        let guard_nfa = if literals.len() == 1 {
            let lit = &literals[0];
            let edit_limits = if let Some(ref lim) = lit.limits {
                let max_edits = lim.get_edits().unwrap_or(0);
                EditLimits::new(max_edits)
            } else {
                EditLimits::new(0)
            };
            Some(GuardNfa::new(&lit.text, edit_limits, case_insensitive))
        } else {
            None
        };

        let has_char_restrictions = edit_char_restrictions
            .iter()
            .any(std::option::Option::is_some);

        Some(FuzzyBridge {
            automata,
            guard_nfa,
            bitap_matchers,
            patterns,
            limits,
            edit_char_restrictions,
            has_char_restrictions,
            case_insensitive,
            search_buffers: RefCell::new(SearchBuffers::new()),
            text_chars_buf: RefCell::new(Vec::new()),
            word_list_patterns: Vec::new(),
            word_list_limits: Vec::new(),
        })
    }

    /// Get the number of patterns.
    #[must_use]
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Returns whether case-insensitive matching is enabled.
    #[must_use]
    pub fn is_case_insensitive(&self) -> bool {
        self.case_insensitive
    }

    /// Add word list patterns (from \L<name>) for matching.
    /// The words will be matched against text with the given fuzzy limits.
    pub fn add_word_list(&mut self, words: &[String], limits: Option<&FuzzyLimits>) {
        for word in words {
            self.word_list_patterns.push(word.clone());
            self.word_list_limits.push(limits.cloned());
        }
    }

    /// Get total pattern count including word lists.
    #[must_use]
    pub fn total_pattern_count(&self) -> usize {
        self.patterns.len() + self.word_list_patterns.len()
    }

    /// Enable minimum-edit end selection on every Bitap matcher
    /// (`MatchEndPolicy::MinEdit`). No-op for patterns that fall back to the NFA
    /// (which already reports the leftmost minimal-edit match).
    pub fn set_prefer_min_edit(&mut self, yes: bool) {
        for bitap in self.bitap_matchers.iter_mut().flatten() {
            bitap.set_prefer_min_edit(yes);
        }
    }

    /// Get the limits for patterns.
    #[must_use]
    pub fn limits(&self) -> &[Option<FuzzyLimits>] {
        &self.limits
    }

    /// Get a Bitap matcher for a pattern index (first pattern).
    #[must_use]
    pub fn get_bitap_matcher(&self) -> Option<&BitapMatcher> {
        self.bitap_matchers.first().and_then(|m| m.as_ref())
    }

    /// Get the character length of a pattern.
    pub fn pattern_char_len(&self, index: usize) -> Option<usize> {
        self.patterns.get(index).map(|s| s.chars().count())
    }

    /// Check if all patterns have 0 max edits (exact matching only).
    /// Returns true if all patterns either have no limits (exact match)
    /// or have explicit limits of 0 edits.
    pub fn is_all_exact(&self) -> bool {
        self.limits.iter().all(|lim| {
            match lim {
                None => true, // No limits = exact match
                Some(lim) => {
                    lim.get_edits().unwrap_or_else(|| {
                        let ins = lim.get_insertions().unwrap_or(0);
                        let del = lim.get_deletions().unwrap_or(0);
                        let sub = lim.get_substitutions().unwrap_or(0);
                        let swap = lim.get_swaps().unwrap_or(0);
                        ins.saturating_add(del)
                            .saturating_add(sub)
                            .saturating_add(swap)
                    }) == 0
                }
            }
        })
    }

    /// Get the maximum edit distance for a pattern.
    pub fn pattern_max_edits(&self, index: usize) -> Option<u8> {
        self.limits.get(index).and_then(|lim| {
            lim.as_ref().map(|l| {
                l.get_edits().unwrap_or_else(|| {
                    let i = l.get_insertions().unwrap_or(0);
                    let d = l.get_deletions().unwrap_or(0);
                    let s = l.get_substitutions().unwrap_or(0);
                    let t = l.get_swaps().unwrap_or(0);
                    i.saturating_add(d).saturating_add(s).saturating_add(t)
                })
            })
        })
    }

    /// Get the maximum pattern length across all patterns.
    pub fn max_pattern_len(&self) -> usize {
        self.patterns
            .iter()
            .map(|p| p.chars().count())
            .max()
            .unwrap_or(0)
    }

    /// Get the maximum edit distance across all patterns.
    pub fn max_edits(&self) -> Option<u8> {
        self.limits
            .iter()
            .filter_map(|lim| {
                lim.as_ref().map(|l| {
                    l.get_edits().unwrap_or_else(|| {
                        let i = l.get_insertions().unwrap_or(0);
                        let d = l.get_deletions().unwrap_or(0);
                        let s = l.get_substitutions().unwrap_or(0);
                        let t = l.get_swaps().unwrap_or(0);
                        i.saturating_add(d).saturating_add(s).saturating_add(t)
                    })
                })
            })
            .max()
    }

    /// Check if all patterns are compatible with Bitap streaming (<=64 chars).
    pub fn all_patterns_bitap_compatible(&self) -> bool {
        self.bitap_matchers.iter().all(Option::is_some)
    }

    /// Search the entire text once and cache all matches.
    /// Uses Bitap when available (faster), falls back to Levenshtein NFA.
    pub fn search_all(&self, text: &str, threshold: f32) -> CachedMatches {
        let mut cached = CachedMatches::default();

        for (pattern_idx, nfa) in self.automata.iter().enumerate() {
            let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

            // Use Bitap when available (faster for short patterns), fall back to NFA
            // with pre-allocated buffers (avoids per-call text_chars Vec allocation).
            let matches = if let Some(ref bitap) = self.bitap_matchers[pattern_idx] {
                let mut buf = self.text_chars_buf.borrow_mut();
                bitap.find_all_buffered(text, pattern_threshold, &mut buf)
            } else {
                let mut buffers = self.search_buffers.borrow_mut();
                nfa.find_all_buffered(text, pattern_threshold, &mut buffers)
            };

            for m in matches {
                // Validate character class restrictions if present
                if let Some(restriction) = self
                    .edit_char_restrictions
                    .get(pattern_idx)
                    .and_then(|r| r.as_ref())
                {
                    let matched_text = &text[m.start..m.end];
                    if !self.validate_edit_chars(
                        &self.patterns[pattern_idx],
                        matched_text,
                        restriction,
                    ) {
                        continue;
                    }
                }

                let result = FuzzyMatchResult {
                    end: m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                };

                cached
                    .by_pattern_and_start
                    .entry((pattern_idx, m.start))
                    .or_default()
                    .push(result);
            }
        }

        // Rank each entry so `get` returns the fewest-edit, longest base
        // alignment (see `cmp_cached_candidates`).
        for matches in cached.by_pattern_and_start.values_mut() {
            matches.sort_by(cmp_cached_candidates);
        }

        cached
    }

    /// Search for fuzzy matches at a specific position and return as `CachedMatches`.
    ///
    /// This is optimized for anchored patterns where we only need to match
    /// at one position (e.g., position 0 for `^` anchored patterns).
    pub fn search_cached_at_position(
        &self,
        text: &str,
        pos: usize,
        threshold: f32,
    ) -> CachedMatches {
        let mut cached = CachedMatches::default();

        if pos >= text.len() {
            return cached;
        }

        let substring = &text[pos..];

        for (pattern_idx, nfa) in self.automata.iter().enumerate() {
            let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

            // Use Bitap when available (faster for short patterns), fall back to NFA
            // with pre-allocated buffers.
            let matches = if let Some(ref bitap) = self.bitap_matchers[pattern_idx] {
                let mut buf = self.text_chars_buf.borrow_mut();
                bitap.find_all_buffered(substring, pattern_threshold, &mut buf)
            } else {
                let mut buffers = self.search_buffers.borrow_mut();
                nfa.find_all_buffered(substring, pattern_threshold, &mut buffers)
            };

            // Only keep matches starting at position 0 of the substring (which is `pos` in original text)
            for m in matches {
                if m.start != 0 {
                    continue;
                }

                // Validate character class restrictions if present
                if let Some(restriction) = self
                    .edit_char_restrictions
                    .get(pattern_idx)
                    .and_then(|r| r.as_ref())
                {
                    let matched_text = &substring[m.start..m.end];
                    if !self.validate_edit_chars(
                        &self.patterns[pattern_idx],
                        matched_text,
                        restriction,
                    ) {
                        continue;
                    }
                }

                let result = FuzzyMatchResult {
                    end: pos + m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                };

                cached
                    .by_pattern_and_start
                    .entry((pattern_idx, pos))
                    .or_default()
                    .push(result);
            }
        }

        // Rank each entry so `get` returns the fewest-edit, longest base
        // alignment (see `cmp_cached_candidates`).
        for matches in cached.by_pattern_and_start.values_mut() {
            matches.sort_by(cmp_cached_candidates);
        }

        cached
    }

    /// Fast non-overlapping search optimized for iteration (greedy leftmost).
    ///
    /// Returns matches directly as a Vec, avoiding the `HashMap` overhead.
    /// Uses optimized Bitap path when available.
    ///
    /// When `require_first_char` is true, matches must start with the same first
    /// character as the pattern. This filters out spurious matches like "bore"
    /// when searching for "Lorem".
    pub fn search_non_overlapping(
        &self,
        text: &str,
        threshold: f32,
        pattern_idx: usize,
        require_first_char: bool,
    ) -> Vec<super::damlev::DamLevMatch> {
        if pattern_idx >= self.automata.len() {
            return Vec::new();
        }

        let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

        // Use optimized Bitap non-overlapping search when available
        if let Some(ref bitap) = self.bitap_matchers[pattern_idx] {
            let matches =
                bitap.find_all_non_overlapping(text, pattern_threshold, require_first_char);

            // Validate character class restrictions if present
            if let Some(restriction) = self
                .edit_char_restrictions
                .get(pattern_idx)
                .and_then(|r| r.as_ref())
            {
                return matches
                    .into_iter()
                    .filter(|m| {
                        let matched_text = &text[m.start..m.end];
                        self.validate_edit_chars(
                            &self.patterns[pattern_idx],
                            matched_text,
                            restriction,
                        )
                    })
                    .collect();
            }

            return matches;
        }

        // Fallback to NFA. `find_all` returns every (overlapping) candidate
        // match; a non-overlapping search must return leftmost, best-per-start,
        // non-overlapping matches. Sort by (start, fewest edits, shortest end)
        // — so the first element equals `search_first`/`find_first` and `find`
        // agrees with `find_iter().next()` — then greedily drop overlaps.
        let mut all = {
            let mut buffers = self.search_buffers.borrow_mut();
            self.automata[pattern_idx].find_all_buffered(text, pattern_threshold, &mut buffers)
        };
        all.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then_with(|| a.total_edits().cmp(&b.total_edits()))
                .then_with(|| a.end.cmp(&b.end))
        });
        let mut result: Vec<super::damlev::DamLevMatch> = Vec::new();
        let mut last_end = 0;
        for m in all {
            if m.start >= last_end {
                last_end = m.end;
                result.push(m);
            }
        }
        result
    }

    /// Find up to `n` non-overlapping matches for a single pattern.
    ///
    /// This is more efficient than `search_non_overlapping` when only a limited
    /// number of matches is needed, as it stops searching after finding `n` matches.
    pub fn search_non_overlapping_n(
        &self,
        text: &str,
        threshold: f32,
        pattern_idx: usize,
        require_first_char: bool,
        n: usize,
    ) -> Vec<super::damlev::DamLevMatch> {
        if n == 0 || pattern_idx >= self.automata.len() {
            return Vec::new();
        }

        let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

        // Use optimized Bitap non-overlapping search with limit when available
        if let Some(ref bitap) = self.bitap_matchers[pattern_idx] {
            let matches =
                bitap.find_n_non_overlapping(text, pattern_threshold, require_first_char, n);

            // Validate character class restrictions if present
            if let Some(restriction) = self
                .edit_char_restrictions
                .get(pattern_idx)
                .and_then(|r| r.as_ref())
            {
                return matches
                    .into_iter()
                    .filter(|m| {
                        let matched_text = &text[m.start..m.end];
                        self.validate_edit_chars(
                            &self.patterns[pattern_idx],
                            matched_text,
                            restriction,
                        )
                    })
                    .collect();
            }

            return matches;
        }

        // Fallback to NFA with limit
        self.automata[pattern_idx].find_n(text, pattern_threshold, n)
    }

    /// Find the first match in text (fast path for single-match queries).
    ///
    /// This is optimized for `find()` calls where only the first match is needed.
    /// Uses early-exit to avoid scanning the entire text after finding a match.
    /// Returns None if no match is found.
    pub fn search_first(
        &self,
        text: &str,
        threshold: f32,
        pattern_idx: usize,
    ) -> Option<super::damlev::DamLevMatch> {
        if pattern_idx >= self.automata.len() {
            return None;
        }

        // Fast path: try exact substring match first.
        //
        // Only sound for NON-fuzzy patterns. With fuzzy edits, a fuzzy match can
        // occur to the LEFT of the first exact occurrence, and `find` must return
        // the leftmost match (to agree with `find_iter`). For example,
        // `(?:test){e<=1}` on "best tset trial test" — the leftmost match is the
        // fuzzy "best" at 0, not the exact "test" at 16. For exact patterns
        // `str::find` already yields the leftmost match, so the shortcut is both
        // correct and faster than Bitap.
        if self.pattern_max_edits(pattern_idx).unwrap_or(0) == 0 {
            let pattern = &self.patterns[pattern_idx];
            if let Some(pos) = text.find(pattern) {
                // Verify exact match meets threshold (always does for similarity=1.0)
                let sim = 1.0f32;
                if sim >= threshold {
                    // Check if pattern text fits within text bounds
                    let end = pos + pattern.len();
                    if end <= text.len() {
                        return Some(super::damlev::DamLevMatch {
                            start: pos,
                            end,
                            insertions: 0,
                            deletions: 0,
                            substitutions: 0,
                            swaps: 0,
                            similarity: sim,
                        });
                    }
                }
            }
        }

        let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

        // Char-class edit restrictions (`{s<=1:[0-9]}`): the first Bitap match
        // may fail the restriction while a LATER match passes. Taking the first
        // match and filtering would return None (or the wrong match), diverging
        // from `find_iter`. Delegate to the same non-overlapping search
        // `find_iter` uses (which filters each match by the restriction) and take
        // the leftmost, so `find` == `find_iter().next()`.
        if self.has_char_restrictions
            && self
                .edit_char_restrictions
                .get(pattern_idx)
                .and_then(|r| r.as_ref())
                .is_some()
        {
            return self
                .search_non_overlapping(text, threshold, pattern_idx, false)
                .into_iter()
                .next();
        }

        // Use Bitap for first match - needed for fuzzy matching
        if let Some(ref bitap) = self.bitap_matchers[pattern_idx] {
            return bitap.find_first_non_overlapping(text, pattern_threshold);
        }

        // Fallback to NFA with pre-allocated buffers
        let mut buffers = self.search_buffers.borrow_mut();
        self.automata[pattern_idx].find_first_buffered(text, pattern_threshold, &mut buffers)
    }

    /// Find best non-overlapping matches, preferring highest similarity.
    ///
    /// This method finds all candidates then selects the best non-overlapping set,
    /// ensuring higher similarity matches are preferred over lower ones.
    ///
    /// When `require_first_char` is true (default), matches must start with the same
    /// first character as the pattern. This filters out spurious matches like "bore"
    /// when searching for "Lorem".
    pub fn search_best_non_overlapping(
        &self,
        text: &str,
        threshold: f32,
        pattern_idx: usize,
        require_first_char: bool,
    ) -> Vec<super::damlev::DamLevMatch> {
        if pattern_idx >= self.automata.len() {
            return Vec::new();
        }

        let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

        // Use optimized Bitap best-match selection when available
        if let Some(ref bitap) = self.bitap_matchers[pattern_idx] {
            let matches =
                bitap.find_best_non_overlapping(text, pattern_threshold, require_first_char);

            // Validate character class restrictions if present
            if let Some(restriction) = self
                .edit_char_restrictions
                .get(pattern_idx)
                .and_then(|r| r.as_ref())
            {
                return matches
                    .into_iter()
                    .filter(|m| {
                        let matched_text = &text[m.start..m.end];
                        self.validate_edit_chars(
                            &self.patterns[pattern_idx],
                            matched_text,
                            restriction,
                        )
                    })
                    .collect();
            }

            return matches;
        }

        // Fallback: get all matches from NFA and select best non-overlapping
        let mut all_matches = {
            let mut buffers = self.search_buffers.borrow_mut();
            self.automata[pattern_idx].find_all_buffered(text, pattern_threshold, &mut buffers)
        };

        // Filter: require first character to match pattern's first char
        // Respects case_insensitive setting
        if require_first_char {
            let pattern = &self.patterns[pattern_idx];
            if let Some(pattern_first) = pattern.chars().next() {
                let case_insensitive = self.case_insensitive;
                all_matches.retain(|m| {
                    if let Some(match_first) = text[m.start..m.end].chars().next() {
                        if case_insensitive {
                            match_first.eq_ignore_ascii_case(&pattern_first)
                        } else {
                            match_first == pattern_first
                        }
                    } else {
                        false
                    }
                });
            }
        }

        // Sort by similarity descending
        all_matches.sort_by(|a, b| match b.similarity.partial_cmp(&a.similarity) {
            Some(std::cmp::Ordering::Equal) | None => a.start.cmp(&b.start),
            Some(ord) => ord,
        });

        // Greedily select non-overlapping
        let mut result = Vec::new();
        let mut occupied = vec![false; text.len() + 1];

        for m in all_matches {
            let overlaps = (m.start..m.end).any(|i| occupied[i]);
            if !overlaps {
                for i in m.start..m.end {
                    occupied[i] = true;
                }
                result.push(m);
            }
        }

        result.sort_by_key(|m| m.start);
        result
    }

    /// Search using prefilter candidates for faster matching.
    ///
    /// Uses prefilter to identify candidate positions, then searches with NFA.
    pub fn search_all_with_prefilter(
        &self,
        text: &str,
        threshold: f32,
        prefilter: &super::prefilter::Prefilter,
    ) -> CachedMatches {
        let mut cached = CachedMatches::default();

        // Collect candidate positions from prefilter
        let max_offset = prefilter.max_offset();
        let candidates: FxHashSet<usize> = prefilter
            .find_candidates(text.as_bytes())
            .flat_map(|pos| pos..=pos.saturating_add(max_offset))
            .collect();

        if candidates.is_empty() {
            return cached;
        }

        let mut buffers = self.search_buffers.borrow_mut();

        for (pattern_idx, nfa) in self.automata.iter().enumerate() {
            let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);
            let matches = nfa.find_all_with_candidates_buffered(
                text,
                pattern_threshold,
                &candidates,
                &mut buffers,
            );

            for m in matches {
                if let Some(restriction) = self
                    .edit_char_restrictions
                    .get(pattern_idx)
                    .and_then(|r| r.as_ref())
                {
                    let matched_text = &text[m.start..m.end];
                    if !self.validate_edit_chars(
                        &self.patterns[pattern_idx],
                        matched_text,
                        restriction,
                    ) {
                        continue;
                    }
                }

                let result = FuzzyMatchResult {
                    end: m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                };

                cached
                    .by_pattern_and_start
                    .entry((pattern_idx, m.start))
                    .or_default()
                    .push(result);
            }
        }

        // Rank each entry so `get` returns the fewest-edit, longest base
        // alignment (see `cmp_cached_candidates`).
        for matches in cached.by_pattern_and_start.values_mut() {
            matches.sort_by(cmp_cached_candidates);
        }

        cached
    }

    /// Find the first match using Guard NFA (fast path for single pattern).
    ///
    /// Returns immediately on first match - optimal for `find_first` operations.
    #[inline]
    pub fn find_first_guard_nfa(
        &self,
        text: &str,
        threshold: f32,
    ) -> Option<(usize, FuzzyMatchResult)> {
        let guard_nfa = self.guard_nfa.as_ref()?;

        guard_nfa.find_first(text, threshold).map(|m| {
            (
                m.start,
                FuzzyMatchResult {
                    end: m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                },
            )
        })
    }

    /// Find the first match for a single-pattern search using prefilter.
    ///
    /// This is optimized for first-match mode: returns as soon as a match is found
    /// without scanning the entire text. Only works for single-pattern searches.
    pub fn find_first_with_prefilter(
        &self,
        text: &str,
        threshold: f32,
        prefilter: &super::prefilter::Prefilter,
    ) -> Option<(usize, FuzzyMatchResult)> {
        if self.automata.len() != 1 {
            return None; // Only works for single pattern
        }

        // Collect candidate positions from prefilter
        let max_offset = prefilter.max_offset();
        let candidates: FxHashSet<usize> = prefilter
            .find_candidates(text.as_bytes())
            .flat_map(|pos| pos..=pos.saturating_add(max_offset))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let pattern_threshold = self.calculate_effective_threshold(0, threshold);

        // Try Bitap first (much faster for short patterns)
        if let Some(ref bitap) = self.bitap_matchers[0]
            && let Some(m) = bitap.find_first_with_candidates(text, pattern_threshold, &candidates)
        {
            // Validate character class restrictions if present
            let restriction = self.edit_char_restrictions.first().and_then(|r| r.as_ref());
            let validation_passed = restriction.is_none_or(|r| {
                let matched_text = &text[m.start..m.end];
                self.validate_edit_chars(&self.patterns[0], matched_text, r)
            });

            if validation_passed {
                return Some((
                    m.start,
                    FuzzyMatchResult {
                        end: m.end,
                        similarity: m.similarity,
                        insertions: m.insertions,
                        deletions: m.deletions,
                        substitutions: m.substitutions,
                        swaps: m.swaps,
                    },
                ));
            }
            // Fall through to NFA if validation failed
        }

        // Fall back to Levenshtein NFA with pre-allocated buffers
        let nfa = &self.automata[0];
        let mut buffers = self.search_buffers.borrow_mut();
        if let Some(m) = nfa.find_first_with_candidates_buffered(
            text,
            pattern_threshold,
            &candidates,
            &mut buffers,
        ) {
            // Validate character class restrictions if present
            if let Some(restriction) = self.edit_char_restrictions.first().and_then(|r| r.as_ref())
            {
                let matched_text = &text[m.start..m.end];
                if !self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                    return None;
                }
            }

            return Some((
                m.start,
                FuzzyMatchResult {
                    end: m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                },
            ));
        }

        None
    }

    /// Search for matches starting from a single position.
    ///
    /// Returns the best match starting at the given position, or None if no match.
    /// This is used for greedy first-match mode.
    pub fn search_at_position(
        &self,
        text: &str,
        start_pos: usize,
        threshold: f32,
    ) -> Option<(usize, FuzzyMatchResult)> {
        if self.automata.len() != 1 {
            return None; // Only works for single pattern
        }

        let pattern_threshold = self.calculate_effective_threshold(0, threshold);

        // Create a single-position candidate set
        let candidates: FxHashSet<usize> = std::iter::once(start_pos).collect();

        // Try Bitap first (much faster for short patterns)
        if let Some(ref bitap) = self.bitap_matchers[0]
            && let Some(m) = bitap.find_first_with_candidates(text, pattern_threshold, &candidates)
            && m.start == start_pos
        {
            // Validate character class restrictions if present
            if let Some(restriction) = self.edit_char_restrictions.first().and_then(|r| r.as_ref())
            {
                let matched_text = &text[m.start..m.end];
                if self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                    return Some((
                        m.start,
                        FuzzyMatchResult {
                            end: m.end,
                            similarity: m.similarity,
                            insertions: m.insertions,
                            deletions: m.deletions,
                            substitutions: m.substitutions,
                            swaps: m.swaps,
                        },
                    ));
                }
            } else {
                return Some((
                    m.start,
                    FuzzyMatchResult {
                        end: m.end,
                        similarity: m.similarity,
                        insertions: m.insertions,
                        deletions: m.deletions,
                        substitutions: m.substitutions,
                        swaps: m.swaps,
                    },
                ));
            }
        }

        // Fall back to Levenshtein NFA
        let nfa = &self.automata[0];

        // Find all matches starting at this position and pick the best
        let mut buffers = self.search_buffers.borrow_mut();
        let matches = nfa.find_all_with_candidates_buffered(
            text,
            pattern_threshold,
            &candidates,
            &mut buffers,
        );

        // Find the best match (highest similarity)
        let best = matches
            .into_iter()
            .filter(|m| m.start == start_pos)
            .max_by(|a, b| {
                a.similarity
                    .partial_cmp(&b.similarity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;

        // Validate character class restrictions if present
        if let Some(restriction) = self.edit_char_restrictions.first().and_then(|r| r.as_ref()) {
            let matched_text = &text[best.start..best.end];
            if !self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                return None;
            }
        }

        Some((
            best.start,
            FuzzyMatchResult {
                end: best.end,
                similarity: best.similarity,
                insertions: best.insertions,
                deletions: best.deletions,
                substitutions: best.substitutions,
                swaps: best.swaps,
            },
        ))
    }

    /// Ultra-fast search at a single position using optimized Bitap.
    ///
    /// This method avoids all allocations for the common case and is
    /// designed for the greedy-first hot path.
    #[inline]
    pub fn search_at_position_fast(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
    ) -> Option<(usize, FuzzyMatchResult)> {
        if self.automata.len() != 1 {
            return None;
        }

        let pattern_threshold = self.calculate_effective_threshold(0, threshold);

        // Use optimized Bitap if available
        if let Some(ref bitap) = self.bitap_matchers[0]
            && let Some(m) = bitap.find_at_byte_position(text, start_pos, pattern_threshold)
        {
            // Skip char restriction validation for speed in common case
            // (restrictions are rare)
            if self
                .edit_char_restrictions
                .first()
                .and_then(|r| r.as_ref())
                .is_none()
            {
                return Some((
                    m.start,
                    FuzzyMatchResult {
                        end: m.end,
                        similarity: m.similarity,
                        insertions: m.insertions,
                        deletions: m.deletions,
                        substitutions: m.substitutions,
                        swaps: m.swaps,
                    },
                ));
            }

            // Validate character class restrictions
            if let Ok(text_str) = std::str::from_utf8(text)
                && let Some(restriction) = &self.edit_char_restrictions[0]
            {
                let matched_text = &text_str[m.start..m.end];
                if self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                    return Some((
                        m.start,
                        FuzzyMatchResult {
                            end: m.end,
                            similarity: m.similarity,
                            insertions: m.insertions,
                            deletions: m.deletions,
                            substitutions: m.substitutions,
                            swaps: m.swaps,
                        },
                    ));
                }
            }
        }

        None
    }

    /// Find first match using streaming Bitap (single-pass, O(n*k)).
    /// Falls back to Boyer-Moore for very short texts where setup overhead matters.
    #[inline]
    pub fn find_first_boyer_moore(
        &self,
        text: &[u8],
        threshold: f32,
        _max_offset: usize,
    ) -> Option<(usize, FuzzyMatchResult)> {
        if self.automata.len() != 1 {
            return None;
        }

        let bitap = self.bitap_matchers[0].as_ref()?;
        let pattern_threshold = self.calculate_effective_threshold(0, threshold);
        let has_restrictions = self
            .edit_char_restrictions
            .first()
            .and_then(|r| r.as_ref())
            .is_some();

        // Use streaming search for better performance on longer texts
        // Streaming is O(n*k) single pass vs Boyer-Moore which is O(n*k*w) with many candidates
        if let Some(m) = bitap.find_first_streaming(text, pattern_threshold) {
            // Quick path: no restrictions
            if !has_restrictions {
                return Some((
                    m.start,
                    FuzzyMatchResult {
                        end: m.end,
                        similarity: m.similarity,
                        insertions: m.insertions,
                        deletions: m.deletions,
                        substitutions: m.substitutions,
                        swaps: m.swaps,
                    },
                ));
            }

            // Validate restrictions
            if let Ok(text_str) = std::str::from_utf8(text)
                && let Some(restriction) = &self.edit_char_restrictions[0]
            {
                let matched_text = &text_str[m.start..m.end];
                if self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                    return Some((
                        m.start,
                        FuzzyMatchResult {
                            end: m.end,
                            similarity: m.similarity,
                            insertions: m.insertions,
                            deletions: m.deletions,
                            substitutions: m.substitutions,
                            swaps: m.swaps,
                        },
                    ));
                }
            }
        }

        None
    }

    /// Find first match using lazy streaming search.
    ///
    /// Processes prefilter candidates one at a time, returning immediately
    /// when a match is found. This is optimal when matches occur early in the text.
    #[inline]
    pub fn find_first_lazy(
        &self,
        text: &[u8],
        threshold: f32,
        prefilter: &super::prefilter::Prefilter,
    ) -> Option<(usize, FuzzyMatchResult)> {
        if self.automata.len() != 1 {
            return None;
        }

        let bitap = self.bitap_matchers.first()?.as_ref()?;
        let pattern_threshold = self.calculate_effective_threshold(0, threshold);
        let max_offset = prefilter.max_offset();
        let has_restrictions = self
            .edit_char_restrictions
            .first()
            .and_then(|r| r.as_ref())
            .is_some();

        // Check if prefilter would be ineffective (e.g., DNA with small alphabet)
        // If searching for 3+ bytes with max_offset > 0, prefilter may generate many false positives
        // In that case, streaming Bitap is more efficient
        let use_streaming = match prefilter {
            super::prefilter::Prefilter::ThreeBytes {
                max_offset: off, ..
            } if *off > 0 => true,
            super::prefilter::Prefilter::MultiBytes {
                max_offset: off, ..
            } if *off > 0 => true,
            _ => false,
        };

        if use_streaming {
            // Fall back to streaming Bitap for better performance
            return self.find_first_boyer_moore(text, threshold, max_offset);
        }

        // Track positions we've already tried to avoid duplicates
        let mut last_tried: Option<usize> = None;

        // Process candidates lazily - return on first match
        for candidate in prefilter.find_candidates(text) {
            // Try positions from (candidate - max_offset) to candidate
            // Search backwards first (most likely match position)
            for back in 0..=max_offset {
                let pos = candidate.saturating_sub(back);

                // Skip UTF-8 continuation bytes
                if pos > 0 && (text[pos] & 0b1100_0000) == 0b1000_0000 {
                    continue;
                }

                // Skip if we already tried this position
                if last_tried == Some(pos) {
                    continue;
                }
                last_tried = Some(pos);

                // Try Bitap at this position
                if let Some(m) = bitap.find_at_byte_position(text, pos, pattern_threshold) {
                    // Quick path: no restrictions
                    if !has_restrictions {
                        return Some((
                            m.start,
                            FuzzyMatchResult {
                                end: m.end,
                                similarity: m.similarity,
                                insertions: m.insertions,
                                deletions: m.deletions,
                                substitutions: m.substitutions,
                                swaps: m.swaps,
                            },
                        ));
                    }

                    // Validate restrictions
                    if let Ok(text_str) = std::str::from_utf8(text)
                        && let Some(restriction) = &self.edit_char_restrictions[0]
                    {
                        let matched_text = &text_str[m.start..m.end];
                        if self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                            return Some((
                                m.start,
                                FuzzyMatchResult {
                                    end: m.end,
                                    similarity: m.similarity,
                                    insertions: m.insertions,
                                    deletions: m.deletions,
                                    substitutions: m.substitutions,
                                    swaps: m.swaps,
                                },
                            ));
                        }
                    }
                }
            }

            // Also try forward positions (for deletions from pattern start)
            for fwd in 1..=max_offset {
                let pos = candidate + fwd;
                if pos >= text.len() {
                    continue;
                }

                // Skip UTF-8 continuation bytes
                if (text[pos] & 0b1100_0000) == 0b1000_0000 {
                    continue;
                }

                if last_tried == Some(pos) {
                    continue;
                }
                last_tried = Some(pos);

                if let Some(m) = bitap.find_at_byte_position(text, pos, pattern_threshold) {
                    if !has_restrictions {
                        return Some((
                            m.start,
                            FuzzyMatchResult {
                                end: m.end,
                                similarity: m.similarity,
                                insertions: m.insertions,
                                deletions: m.deletions,
                                substitutions: m.substitutions,
                                swaps: m.swaps,
                            },
                        ));
                    }

                    if let Ok(text_str) = std::str::from_utf8(text)
                        && let Some(restriction) = &self.edit_char_restrictions[0]
                    {
                        let matched_text = &text_str[m.start..m.end];
                        if self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                            return Some((
                                m.start,
                                FuzzyMatchResult {
                                    end: m.end,
                                    similarity: m.similarity,
                                    insertions: m.insertions,
                                    deletions: m.deletions,
                                    substitutions: m.substitutions,
                                    swaps: m.swaps,
                                },
                            ));
                        }
                    }
                }
            }
        }

        None
    }

    /// Find first match using batch parallel position search.
    ///
    /// Collects candidate positions from prefilter and processes them in batches
    /// using SIMD multi-position search for improved throughput.
    ///
    /// This is most effective for:
    /// - k=0 (exact match) where SIMD can process 2-4 positions per iteration
    /// - ASCII patterns
    /// - Many candidate positions
    #[inline]
    pub fn find_first_batch_parallel(
        &self,
        text: &[u8],
        threshold: f32,
        prefilter: &super::prefilter::Prefilter,
    ) -> Option<(usize, FuzzyMatchResult)> {
        if self.automata.len() != 1 {
            return None;
        }

        let bitap = self.bitap_matchers.first()?.as_ref()?;
        let pattern_threshold = self.calculate_effective_threshold(0, threshold);
        let max_offset = prefilter.max_offset();

        // Collect candidate positions
        // Must search BOTH directions from candidate:
        // - Forward: for deletions from pattern start
        // - Backward: for insertions before pattern start
        let mut positions: Vec<usize> = Vec::with_capacity(64);
        let mut seen: FxHashSet<usize> = FxHashSet::default();

        for candidate in prefilter.find_candidates(text) {
            // Search backwards (for insertions at match start)
            for back in 0..=max_offset {
                let pos = candidate.saturating_sub(back);
                if pos > 0 && (text[pos] & 0b1100_0000) == 0b1000_0000 {
                    continue;
                }
                if seen.insert(pos) {
                    positions.push(pos);
                }
            }
            // Search forwards (for deletions from pattern start)
            for fwd in 1..=max_offset {
                let pos = candidate + fwd;
                if pos >= text.len() {
                    continue;
                }
                if pos > 0 && (text[pos] & 0b1100_0000) == 0b1000_0000 {
                    continue;
                }
                if seen.insert(pos) {
                    positions.push(pos);
                }
            }
        }

        if positions.is_empty() {
            return None;
        }

        // Use batch parallel search
        if let Some((_idx, m)) =
            bitap.find_at_positions_parallel(text, &positions, pattern_threshold)
        {
            // Skip restriction validation in fast path (rare case)
            if self
                .edit_char_restrictions
                .first()
                .and_then(|r| r.as_ref())
                .is_none()
            {
                return Some((
                    m.start,
                    FuzzyMatchResult {
                        end: m.end,
                        similarity: m.similarity,
                        insertions: m.insertions,
                        deletions: m.deletions,
                        substitutions: m.substitutions,
                        swaps: m.swaps,
                    },
                ));
            }

            // Validate edit char restrictions
            if let Ok(text_str) = std::str::from_utf8(text)
                && let Some(restriction) =
                    self.edit_char_restrictions.first().and_then(|r| r.as_ref())
            {
                let matched_text = &text_str[m.start..m.end];
                if self.validate_edit_chars(&self.patterns[0], matched_text, restriction) {
                    return Some((
                        m.start,
                        FuzzyMatchResult {
                            end: m.end,
                            similarity: m.similarity,
                            insertions: m.insertions,
                            deletions: m.deletions,
                            substitutions: m.substitutions,
                            swaps: m.swaps,
                        },
                    ));
                }
            }
        }

        None
    }

    /// Find first match across multiple patterns using parallel Bitap search.
    ///
    /// This is optimized for simple alternations where we can skip NFA simulation
    /// and just run Bitap for each pattern at candidate positions.
    ///
    /// Returns (`pattern_index`, start, `FuzzyMatchResult`) for the first match found.
    pub fn find_first_multi_pattern(
        &self,
        text: &[u8],
        threshold: f32,
        pattern_indices: &[usize],
        prefilter: &super::prefilter::Prefilter,
    ) -> Option<(usize, usize, FuzzyMatchResult)> {
        if pattern_indices.is_empty() {
            return None;
        }

        // Fast path for exact matches: use memmem instead of Bitap
        // Check if all patterns have max_edits = 0
        let all_exact = pattern_indices
            .iter()
            .all(|&idx| self.pattern_max_edits(idx).unwrap_or(0) == 0);

        if all_exact {
            // Fast path for exact matches: use prefilter candidates + memcmp
            // This avoids scanning the entire text for each pattern
            let mut best: Option<(usize, usize, FuzzyMatchResult)> = None;

            // Get candidate positions from prefilter
            let candidates: Vec<usize> = prefilter.find_candidates(text).collect();

            for &candidate in &candidates {
                // Try each pattern at this position
                for &pattern_idx in pattern_indices {
                    if let Some(pattern_text) = self.pattern_text(pattern_idx) {
                        let pattern_bytes = pattern_text.as_bytes();
                        let pos = candidate;

                        // Check if pattern fits at this position
                        if pos + pattern_bytes.len() <= text.len() {
                            // Use memcmp for fast comparison
                            if text[pos..pos + pattern_bytes.len()] == *pattern_bytes {
                                let result = FuzzyMatchResult {
                                    end: pos + pattern_bytes.len(),
                                    similarity: 1.0,
                                    insertions: 0,
                                    deletions: 0,
                                    substitutions: 0,
                                    swaps: 0,
                                };

                                if best
                                    .as_ref()
                                    .is_none_or(|(_, best_start, _)| pos < *best_start)
                                {
                                    best = Some((pattern_idx, pos, result));
                                    if pos == 0 {
                                        return best;
                                    }
                                }
                                break; // Found match at this position, try next candidate
                            }
                        }
                    }
                }
            }

            return best;
        }

        let text_str = std::str::from_utf8(text).ok()?;
        let max_offset = prefilter.max_offset();

        // Iterate through candidate positions from prefilter
        let mut last_tried: Option<usize> = None;

        for candidate in prefilter.find_candidates(text) {
            // Try positions from candidate to candidate + max_offset
            for offset in 0..=max_offset {
                let pos = candidate + offset;
                if pos >= text.len() {
                    continue;
                }

                // Skip if not on a char boundary or already tried
                if pos > 0 && (text[pos] & 0b1100_0000) == 0b1000_0000 {
                    continue;
                }
                if last_tried == Some(pos) {
                    continue;
                }
                last_tried = Some(pos);

                // Try each pattern at this position
                for &pattern_idx in pattern_indices {
                    if pattern_idx >= self.bitap_matchers.len() {
                        continue;
                    }

                    let pattern_threshold =
                        self.calculate_effective_threshold(pattern_idx, threshold);

                    // Try Bitap first (fast path)
                    if let Some(ref bitap) = self.bitap_matchers[pattern_idx]
                        && let Some(m) = bitap.find_at_byte_position(text, pos, pattern_threshold)
                    {
                        // Check edit char restrictions
                        if let Some(restriction) = self
                            .edit_char_restrictions
                            .get(pattern_idx)
                            .and_then(|r| r.as_ref())
                        {
                            let matched_text = &text_str[m.start..m.end];
                            if !self.validate_edit_chars(
                                &self.patterns[pattern_idx],
                                matched_text,
                                restriction,
                            ) {
                                continue;
                            }
                        }

                        return Some((
                            pattern_idx,
                            m.start,
                            FuzzyMatchResult {
                                end: m.end,
                                similarity: m.similarity,
                                insertions: m.insertions,
                                deletions: m.deletions,
                                substitutions: m.substitutions,
                                swaps: m.swaps,
                            },
                        ));
                    }

                    // Fallback to Levenshtein NFA for patterns > 64 chars
                    if self
                        .bitap_matchers
                        .get(pattern_idx)
                        .is_none_or(Option::is_none)
                    {
                        let nfa = &self.automata[pattern_idx];
                        let candidates: FxHashSet<usize> = std::iter::once(pos).collect();
                        let mut buffers = self.search_buffers.borrow_mut();
                        if let Some(m) = nfa.find_first_with_candidates_buffered(
                            text_str,
                            pattern_threshold,
                            &candidates,
                            &mut buffers,
                        ) && m.start == pos
                        {
                            // Check edit char restrictions
                            if let Some(restriction) = self
                                .edit_char_restrictions
                                .get(pattern_idx)
                                .and_then(|r| r.as_ref())
                            {
                                let matched_text = &text_str[m.start..m.end];
                                if !self.validate_edit_chars(
                                    &self.patterns[pattern_idx],
                                    matched_text,
                                    restriction,
                                ) {
                                    continue;
                                }
                            }

                            return Some((
                                pattern_idx,
                                m.start,
                                FuzzyMatchResult {
                                    end: m.end,
                                    similarity: m.similarity,
                                    insertions: m.insertions,
                                    deletions: m.deletions,
                                    substitutions: m.substitutions,
                                    swaps: m.swaps,
                                },
                            ));
                        }
                    }
                }
            }
        }

        None
    }

    /// Find the first match across multiple patterns by running each pattern's
    /// streaming search individually. This is used when the multi-pattern prefilter
    /// would be ineffective (too many common bytes).
    ///
    /// Returns (`pattern_index`, start, `FuzzyMatchResult`) for the earliest match found.
    pub fn find_first_multi_pattern_individual(
        &self,
        text: &[u8],
        threshold: f32,
        pattern_indices: &[usize],
    ) -> Option<(usize, usize, FuzzyMatchResult)> {
        if pattern_indices.is_empty() {
            return None;
        }

        let mut best: Option<(usize, usize, FuzzyMatchResult)> = None;

        // Run each pattern's streaming search individually
        for &pattern_idx in pattern_indices {
            if pattern_idx >= self.bitap_matchers.len() {
                continue;
            }

            let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

            // Use Bitap streaming for each pattern
            let Some(bitap) = self.bitap_matchers[pattern_idx].as_ref() else {
                continue;
            };

            // Try Bitap streaming first
            if let Some(m) = bitap.find_first_streaming(text, pattern_threshold) {
                // Check edit char restrictions
                let restriction = self
                    .edit_char_restrictions
                    .get(pattern_idx)
                    .and_then(|r| r.as_ref());
                let validation_passed = match (restriction, std::str::from_utf8(text)) {
                    (Some(r), Ok(text_str)) => {
                        let matched_text = &text_str[m.start..m.end];
                        self.validate_edit_chars(&self.patterns[pattern_idx], matched_text, r)
                    }
                    _ => true, // No restriction or invalid UTF-8, pass validation
                };

                if validation_passed {
                    let result = FuzzyMatchResult {
                        end: m.end,
                        similarity: m.similarity,
                        insertions: m.insertions,
                        deletions: m.deletions,
                        substitutions: m.substitutions,
                        swaps: m.swaps,
                    };
                    // Only prefer earlier start position; for same position, first pattern wins
                    // (matches mrab-regex behavior where pattern order matters)
                    if best
                        .as_ref()
                        .is_none_or(|(_, best_start, _)| m.start < *best_start)
                    {
                        best = Some((pattern_idx, m.start, result));
                        // Early termination: if we found a match at position 0, no need to check other patterns
                        if m.start == 0 {
                            return best;
                        }
                    }
                    continue;
                }
                // Fall through to NFA if validation failed
            }

            // Fallback to Levenshtein NFA if Bitap didn't find a match
            if pattern_idx < self.automata.len() {
                let nfa = &self.automata[pattern_idx];
                if let Ok(text_str) = std::str::from_utf8(text) {
                    // Find all matches and take the earliest one
                    let mut buffers = self.search_buffers.borrow_mut();
                    let matches = nfa.find_all_buffered(text_str, pattern_threshold, &mut buffers);
                    if let Some(m) = matches.into_iter().min_by_key(|m| m.start) {
                        // Check edit char restrictions
                        if let Some(restriction) = self
                            .edit_char_restrictions
                            .get(pattern_idx)
                            .and_then(|r| r.as_ref())
                        {
                            let matched_text = &text_str[m.start..m.end];
                            if !self.validate_edit_chars(
                                &self.patterns[pattern_idx],
                                matched_text,
                                restriction,
                            ) {
                                continue;
                            }
                        }

                        let result = FuzzyMatchResult {
                            end: m.end,
                            similarity: m.similarity,
                            insertions: m.insertions,
                            deletions: m.deletions,
                            substitutions: m.substitutions,
                            swaps: m.swaps,
                        };
                        // Only prefer earlier start position; for same position, first pattern wins
                        if best
                            .as_ref()
                            .is_none_or(|(_, best_start, _)| m.start < *best_start)
                        {
                            best = Some((pattern_idx, m.start, result));
                            // Early termination: if we found a match at position 0, no need to check other patterns
                            if m.start == 0 {
                                return best;
                            }
                        }
                    }
                }
            }
        }

        best
    }

    /// Calculate the minimum effective threshold across all patterns.
    ///
    /// This returns the lowest threshold that could match any pattern,
    /// useful for early-exit optimizations.
    #[must_use]
    pub fn calculate_min_effective_threshold(&self, user_threshold: f32) -> f32 {
        let mut min_threshold = user_threshold;

        for idx in 0..self.patterns.len() {
            let pattern_threshold = self.calculate_effective_threshold(idx, user_threshold);
            if pattern_threshold < min_threshold {
                min_threshold = pattern_threshold;
            }
        }

        min_threshold
    }

    /// Search for all matches across multiple patterns efficiently.
    ///
    /// This is optimized for the multi-pattern case by processing all patterns
    /// in parallel, avoiding redundant text scans.
    ///
    /// Returns a map of (`pattern_index`, start) -> `Vec<FuzzyMatchResult>`.
    pub fn search_all_multi_pattern(
        &self,
        text: &str,
        threshold: f32,
        pattern_indices: &[usize],
    ) -> CachedMatches {
        let mut cached = CachedMatches::default();

        if pattern_indices.is_empty() {
            return cached;
        }

        // For small number of patterns, parallel individual search is efficient
        // For larger pattern sets, a true Aho-Corasick automaton would be better
        for &pattern_idx in pattern_indices {
            if pattern_idx >= self.automata.len() {
                continue;
            }

            let pattern_threshold = self.calculate_effective_threshold(pattern_idx, threshold);

            // Use Bitap when available (faster for short patterns)
            let matches = if let Some(bitap) = self
                .bitap_matchers
                .get(pattern_idx)
                .and_then(|b| b.as_ref())
            {
                let mut buf = self.text_chars_buf.borrow_mut();
                bitap.find_all_buffered(text, pattern_threshold, &mut buf)
            } else {
                let mut buffers = self.search_buffers.borrow_mut();
                self.automata[pattern_idx].find_all_buffered(text, pattern_threshold, &mut buffers)
            };

            for m in matches {
                // Validate character class restrictions if present
                if let Some(restriction) = self
                    .edit_char_restrictions
                    .get(pattern_idx)
                    .and_then(|r| r.as_ref())
                {
                    let matched_text = &text[m.start..m.end];
                    if !self.validate_edit_chars(
                        &self.patterns[pattern_idx],
                        matched_text,
                        restriction,
                    ) {
                        continue;
                    }
                }

                let result = FuzzyMatchResult {
                    end: m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                };

                cached
                    .by_pattern_and_start
                    .entry((pattern_idx, m.start))
                    .or_default()
                    .push(result);
            }
        }

        // Sort each entry by similarity (highest first)
        for matches in cached.by_pattern_and_start.values_mut() {
            matches.sort_by(|a, b| {
                b.similarity
                    .partial_cmp(&a.similarity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        cached
    }

    /// Find a fuzzy match using cached results.
    pub fn find_at_cached(
        &self,
        cached: &CachedMatches,
        pattern_index: usize,
        from: usize,
    ) -> Option<FuzzyMatchResult> {
        cached.get(pattern_index, from).cloned()
    }

    /// Find a fuzzy match for a specific pattern at a position.
    pub fn find_at(
        &self,
        text: &str,
        pattern_index: usize,
        from: usize,
        threshold: f32,
    ) -> Option<FuzzyMatchResult> {
        let pattern = self.patterns.get(pattern_index)?;

        // Fast path for exact matching (no fuzzy edits allowed)
        // Just check if the pattern is a prefix of the substring
        let max_edits = self.limits.get(pattern_index).and_then(|lim| {
            lim.as_ref().map(|l| {
                l.get_edits().unwrap_or_else(|| {
                    let i = l.get_insertions().unwrap_or(0);
                    let d = l.get_deletions().unwrap_or(0);
                    let s = l.get_substitutions().unwrap_or(0);
                    let t = l.get_swaps().unwrap_or(0);
                    i.saturating_add(d).saturating_add(s).saturating_add(t)
                })
            })
        });

        // Handle empty/exhausted text: can still match via pure deletions
        if from >= text.len() {
            // Check if we can delete the entire pattern
            let pattern_char_len = pattern.chars().count();
            if let Some(max) = max_edits
                && pattern_char_len <= max as usize
            {
                // Calculate similarity for deleting entire pattern
                let deletions = pattern_char_len as u8;
                let max_len = pattern_char_len.max(1) as f32;
                let sim = (1.0 - f32::from(deletions) / max_len).max(0.0);
                if sim >= threshold {
                    return Some(FuzzyMatchResult {
                        end: from,
                        similarity: sim,
                        insertions: 0,
                        deletions,
                        substitutions: 0,
                        swaps: 0,
                    });
                }
            }
            return None;
        }

        let substring = &text[from..];

        if max_edits.is_none() || max_edits == Some(0) {
            // Exact matching - just check if pattern is prefix
            if self.case_insensitive {
                if substring
                    .chars()
                    .take(pattern.chars().count())
                    .zip(pattern.chars())
                    .all(|(t, p)| t.to_lowercase().eq(p.to_lowercase()))
                {
                    let end_byte = pattern.len().min(substring.len());
                    // Make sure we end on a char boundary
                    let actual_end = if end_byte <= substring.len() {
                        let mut e = end_byte;
                        while e < substring.len() && !substring.is_char_boundary(e) {
                            e += 1;
                        }
                        e.min(substring.len())
                    } else {
                        end_byte
                    };
                    return Some(FuzzyMatchResult {
                        end: from + actual_end,
                        similarity: 1.0,
                        insertions: 0,
                        deletions: 0,
                        substitutions: 0,
                        swaps: 0,
                    });
                }
            } else if substring.starts_with(pattern) {
                return Some(FuzzyMatchResult {
                    end: from + pattern.len(),
                    similarity: 1.0,
                    insertions: 0,
                    deletions: 0,
                    substitutions: 0,
                    swaps: 0,
                });
            }
            return None;
        }

        // Fuzzy matching path - need to search
        let nfa = self.automata.get(pattern_index)?;
        let effective_threshold = self.calculate_effective_threshold(pattern_index, threshold);
        let mut buffers = self.search_buffers.borrow_mut();
        let matches = nfa.find_all_buffered(substring, effective_threshold, &mut buffers);

        // Find match that starts at position 0 of the substring
        for m in matches {
            if m.start == 0 {
                // Validate character class restrictions
                if let Some(restriction) = self
                    .edit_char_restrictions
                    .get(pattern_index)
                    .and_then(|r| r.as_ref())
                {
                    let matched_text = &substring[m.start..m.end];
                    if !self.validate_edit_chars(
                        &self.patterns[pattern_index],
                        matched_text,
                        restriction,
                    ) {
                        continue;
                    }
                }

                return Some(FuzzyMatchResult {
                    end: from + m.end,
                    similarity: m.similarity,
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                });
            }
        }

        None
    }

    /// Whether `ch` may be used as a boundary insertion for `pattern_index`.
    ///
    /// Returns `true` when the pattern has no edit-character restriction, or when
    /// the restriction allows `ch`. Used to gate trailing insertions emitted at a
    /// fuzzy literal's boundary so they honour `{i<=N:[...]}` restrictions.
    #[must_use]
    pub fn boundary_insertion_allowed(&self, pattern_index: usize, ch: char) -> bool {
        self.edit_char_restrictions
            .get(pattern_index)
            .and_then(|r| r.as_ref())
            .is_none_or(|restriction| restriction.allows(ch))
    }

    /// Find a fuzzy match that allows boundary insertions (for anchored patterns).
    /// Uses cached results to avoid O(N) per-call overhead.
    pub fn find_with_boundary_insertions(
        &self,
        text: &str,
        pattern_index: usize,
        from: usize,
        to: Option<usize>,
        threshold: f32,
        cached: Option<&CachedMatches>,
    ) -> Option<FuzzyMatchResult> {
        // If no cache, return None (don't do expensive search)
        let cached = cached?;

        let limits = self.limits.get(pattern_index).and_then(|l| l.as_ref())?;
        let max_edits_val = limits.get_edits().unwrap_or_else(|| {
            let i = limits.get_insertions().unwrap_or(0);
            let d = limits.get_deletions().unwrap_or(0);
            let s = limits.get_substitutions().unwrap_or(0);
            let t = limits.get_swaps().unwrap_or(0);
            i.saturating_add(d).saturating_add(s).saturating_add(t)
        });
        let max_insertions = limits.get_insertions().unwrap_or(max_edits_val);

        let effective_threshold = self.calculate_effective_threshold(pattern_index, threshold);

        // Look for matches within the boundary window
        // A match starting before `from` could still be extended to include `from`
        let max_window = max_insertions as usize;
        let search_start = from.saturating_sub(max_window);

        // Collect potential matches from cache
        let matches: Vec<_> = (search_start..=from)
            .filter_map(|start| {
                cached
                    .get_all(pattern_index, start)
                    .map(|results| results.iter().map(move |r| (start, r)))
            })
            .flatten()
            .collect();

        let mut best: Option<FuzzyMatchResult> = None;

        for (match_start, m) in matches {
            // Calculate boundary insertions
            // start_insertions: characters between match_start and from
            let start_insertions = (from.saturating_sub(match_start)) as u8;
            // end_insertions: characters between match end and expected end
            let end_insertions = if let Some(expected_end) = to {
                if m.end < expected_end {
                    (expected_end - m.end) as u8
                } else {
                    0
                }
            } else {
                0
            };

            let total_boundary_insertions = start_insertions + end_insertions;
            let total_insertions = m.insertions + total_boundary_insertions;
            let total_edits =
                m.insertions + m.deletions + m.substitutions + total_boundary_insertions;

            if total_edits > max_edits_val || total_insertions > max_insertions {
                continue;
            }

            // Validate character class restrictions
            if let Some(restriction) = self
                .edit_char_restrictions
                .get(pattern_index)
                .and_then(|r| r.as_ref())
            {
                // Validate boundary chars at start
                let mut boundary_valid = true;
                if start_insertions > 0 && match_start < from {
                    for ch in text[match_start..from].chars() {
                        if !restriction.allows(ch) {
                            boundary_valid = false;
                            break;
                        }
                    }
                }
                // Validate boundary chars at end
                if boundary_valid
                    && end_insertions > 0
                    && let Some(expected_end) = to
                    && m.end < expected_end
                    && m.end < text.len()
                {
                    let end_slice_end = expected_end.min(text.len());
                    for ch in text[m.end..end_slice_end].chars() {
                        if !restriction.allows(ch) {
                            boundary_valid = false;
                            break;
                        }
                    }
                }
                if !boundary_valid {
                    continue;
                }
            }

            // Calculate adjusted similarity
            let pattern_len = self.patterns[pattern_index].chars().count() as f32;
            let insertion_penalty = 0.5;
            let boundary_penalty = f32::from(total_boundary_insertions) * insertion_penalty;
            let adjusted_similarity = if pattern_len > 0.0 {
                ((pattern_len - boundary_penalty) / pattern_len).max(0.0) * m.similarity
            } else {
                m.similarity
            };

            if adjusted_similarity < effective_threshold {
                continue;
            }

            let result = FuzzyMatchResult {
                end: to.unwrap_or(m.end),
                similarity: adjusted_similarity,
                insertions: total_insertions,
                deletions: m.deletions,
                substitutions: m.substitutions,
                swaps: m.swaps,
            };

            if best.as_ref().is_none_or(|b| {
                result.similarity > b.similarity
                    || (result.similarity == b.similarity && result.total_edits() < b.total_edits())
            }) {
                best = Some(result);
            }
        }

        best
    }

    /// Calculate an effective threshold.
    ///
    /// The user's threshold is always respected - both constraints must be satisfied:
    /// - similarity >= `user_threshold`
    /// - edits <= `max_edits` (from pattern syntax)
    #[allow(clippy::unused_self)]
    fn calculate_effective_threshold(&self, _pattern_index: usize, user_threshold: f32) -> f32 {
        // Previously this function tried to lower the threshold based on max_edits,
        // but this was incorrect - it allowed low-quality matches that the user
        // didn't want. The user's threshold should always be respected.
        user_threshold
    }

    /// Get the pattern text for a given index.
    pub fn pattern_text(&self, index: usize) -> Option<&str> {
        self.patterns.get(index).map(String::as_str)
    }

    /// Validate that all edit characters conform to the restriction.
    /// Uses Damerau-Levenshtein to properly detect transpositions.
    /// Optimized with ASCII fast path and stack allocation for small strings.
    fn validate_edit_chars(
        &self,
        pattern: &str,
        matched_text: &str,
        restriction: &EditCharRestriction,
    ) -> bool {
        // Fast path: exact match needs no validation
        if pattern == matched_text {
            return true;
        }

        // ASCII fast path (common case) - avoids Vec<char> allocation
        if pattern.is_ascii() && matched_text.is_ascii() {
            return self.validate_edit_chars_ascii(
                pattern.as_bytes(),
                matched_text.as_bytes(),
                restriction,
            );
        }

        // Unicode path
        let pattern_chars: Vec<char> = pattern.chars().collect();
        let text_chars: Vec<char> = matched_text.chars().collect();
        self.validate_edit_chars_slice(&pattern_chars, &text_chars, restriction)
    }

    /// ASCII-optimized validation (no char conversion needed).
    #[inline]
    #[allow(clippy::unused_self)]
    fn validate_edit_chars_ascii(
        &self,
        pattern: &[u8],
        text: &[u8],
        restriction: &EditCharRestriction,
    ) -> bool {
        let m = pattern.len();
        let n = text.len();

        #[derive(Clone, Copy)]
        enum Op {
            None,
            Insert,
            Delete,
            Substitute,
            Transpose,
        }

        // Stack allocation for small strings (covers most cases)
        const STACK_LIMIT: usize = 32;
        if m < STACK_LIMIT && n < STACK_LIMIT {
            let mut dp = [[(0usize, Op::None); STACK_LIMIT]; STACK_LIMIT];

            for i in 1..=m {
                dp[i][0] = (i, Op::Delete);
            }
            for j in 1..=n {
                dp[0][j] = (j, Op::Insert);
            }

            for i in 1..=m {
                for j in 1..=n {
                    if pattern[i - 1] == text[j - 1] {
                        dp[i][j] = (dp[i - 1][j - 1].0, Op::None);
                    } else {
                        let sub = dp[i - 1][j - 1].0 + 1;
                        let del = dp[i - 1][j].0 + 1;
                        let ins = dp[i][j - 1].0 + 1;

                        let trans = if i > 1
                            && j > 1
                            && pattern[i - 1] == text[j - 2]
                            && pattern[i - 2] == text[j - 1]
                        {
                            dp[i - 2][j - 2].0 + 1
                        } else {
                            usize::MAX
                        };

                        let mut best = (sub, Op::Substitute);
                        if del < best.0 {
                            best = (del, Op::Delete);
                        }
                        if ins < best.0 {
                            best = (ins, Op::Insert);
                        }
                        if trans < best.0 {
                            best = (trans, Op::Transpose);
                        }
                        dp[i][j] = best;
                    }
                }
            }

            // Backtrack and validate
            let (mut i, mut j) = (m, n);
            while i > 0 || j > 0 {
                match dp[i][j].1 {
                    Op::None => {
                        i -= 1;
                        j -= 1;
                    }
                    Op::Substitute => {
                        if !restriction.allows(text[j - 1] as char) {
                            return false;
                        }
                        i -= 1;
                        j -= 1;
                    }
                    Op::Delete => {
                        i -= 1;
                    }
                    Op::Insert => {
                        if !restriction.allows(text[j - 1] as char) {
                            return false;
                        }
                        j -= 1;
                    }
                    Op::Transpose => {
                        i -= 2;
                        j -= 2;
                    }
                }
            }
            return true;
        }

        // Heap allocation for larger strings
        let mut dp = vec![vec![(0usize, Op::None); n + 1]; m + 1];

        for i in 1..=m {
            dp[i][0] = (i, Op::Delete);
        }
        for j in 1..=n {
            dp[0][j] = (j, Op::Insert);
        }

        for i in 1..=m {
            for j in 1..=n {
                if pattern[i - 1] == text[j - 1] {
                    dp[i][j] = (dp[i - 1][j - 1].0, Op::None);
                } else {
                    let sub = dp[i - 1][j - 1].0 + 1;
                    let del = dp[i - 1][j].0 + 1;
                    let ins = dp[i][j - 1].0 + 1;

                    let trans = if i > 1
                        && j > 1
                        && pattern[i - 1] == text[j - 2]
                        && pattern[i - 2] == text[j - 1]
                    {
                        dp[i - 2][j - 2].0 + 1
                    } else {
                        usize::MAX
                    };

                    let mut best = (sub, Op::Substitute);
                    if del < best.0 {
                        best = (del, Op::Delete);
                    }
                    if ins < best.0 {
                        best = (ins, Op::Insert);
                    }
                    if trans < best.0 {
                        best = (trans, Op::Transpose);
                    }
                    dp[i][j] = best;
                }
            }
        }

        let (mut i, mut j) = (m, n);
        while i > 0 || j > 0 {
            match dp[i][j].1 {
                Op::None => {
                    i -= 1;
                    j -= 1;
                }
                Op::Substitute => {
                    if !restriction.allows(text[j - 1] as char) {
                        return false;
                    }
                    i -= 1;
                    j -= 1;
                }
                Op::Delete => {
                    i -= 1;
                }
                Op::Insert => {
                    if !restriction.allows(text[j - 1] as char) {
                        return false;
                    }
                    j -= 1;
                }
                Op::Transpose => {
                    i -= 2;
                    j -= 2;
                }
            }
        }
        true
    }

    /// Unicode validation using char slices.
    #[inline]
    #[allow(clippy::unused_self)]
    fn validate_edit_chars_slice(
        &self,
        pattern: &[char],
        text: &[char],
        restriction: &EditCharRestriction,
    ) -> bool {
        let m = pattern.len();
        let n = text.len();

        #[derive(Clone, Copy)]
        enum Op {
            None,
            Insert,
            Delete,
            Substitute,
            Transpose,
        }

        // Stack allocation for small strings
        const STACK_LIMIT: usize = 32;
        if m < STACK_LIMIT && n < STACK_LIMIT {
            let mut dp = [[(0usize, Op::None); STACK_LIMIT]; STACK_LIMIT];

            for i in 1..=m {
                dp[i][0] = (i, Op::Delete);
            }
            for j in 1..=n {
                dp[0][j] = (j, Op::Insert);
            }

            for i in 1..=m {
                for j in 1..=n {
                    if pattern[i - 1] == text[j - 1] {
                        dp[i][j] = (dp[i - 1][j - 1].0, Op::None);
                    } else {
                        let sub = dp[i - 1][j - 1].0 + 1;
                        let del = dp[i - 1][j].0 + 1;
                        let ins = dp[i][j - 1].0 + 1;

                        let trans = if i > 1
                            && j > 1
                            && pattern[i - 1] == text[j - 2]
                            && pattern[i - 2] == text[j - 1]
                        {
                            dp[i - 2][j - 2].0 + 1
                        } else {
                            usize::MAX
                        };

                        let mut best = (sub, Op::Substitute);
                        if del < best.0 {
                            best = (del, Op::Delete);
                        }
                        if ins < best.0 {
                            best = (ins, Op::Insert);
                        }
                        if trans < best.0 {
                            best = (trans, Op::Transpose);
                        }
                        dp[i][j] = best;
                    }
                }
            }

            let (mut i, mut j) = (m, n);
            while i > 0 || j > 0 {
                match dp[i][j].1 {
                    Op::None => {
                        i -= 1;
                        j -= 1;
                    }
                    Op::Substitute => {
                        if !restriction.allows(text[j - 1]) {
                            return false;
                        }
                        i -= 1;
                        j -= 1;
                    }
                    Op::Delete => {
                        i -= 1;
                    }
                    Op::Insert => {
                        if !restriction.allows(text[j - 1]) {
                            return false;
                        }
                        j -= 1;
                    }
                    Op::Transpose => {
                        i -= 2;
                        j -= 2;
                    }
                }
            }
            return true;
        }

        // Heap allocation for larger strings
        let mut dp = vec![vec![(0usize, Op::None); n + 1]; m + 1];

        for i in 1..=m {
            dp[i][0] = (i, Op::Delete);
        }
        for j in 1..=n {
            dp[0][j] = (j, Op::Insert);
        }

        for i in 1..=m {
            for j in 1..=n {
                if pattern[i - 1] == text[j - 1] {
                    dp[i][j] = (dp[i - 1][j - 1].0, Op::None);
                } else {
                    let sub = dp[i - 1][j - 1].0 + 1;
                    let del = dp[i - 1][j].0 + 1;
                    let ins = dp[i][j - 1].0 + 1;

                    let trans = if i > 1
                        && j > 1
                        && pattern[i - 1] == text[j - 2]
                        && pattern[i - 2] == text[j - 1]
                    {
                        dp[i - 2][j - 2].0 + 1
                    } else {
                        usize::MAX
                    };

                    let mut best = (sub, Op::Substitute);
                    if del < best.0 {
                        best = (del, Op::Delete);
                    }
                    if ins < best.0 {
                        best = (ins, Op::Insert);
                    }
                    if trans < best.0 {
                        best = (trans, Op::Transpose);
                    }
                    dp[i][j] = best;
                }
            }
        }

        let (mut i, mut j) = (m, n);
        while i > 0 || j > 0 {
            match dp[i][j].1 {
                Op::None => {
                    i -= 1;
                    j -= 1;
                }
                Op::Substitute => {
                    if !restriction.allows(text[j - 1]) {
                        return false;
                    }
                    i -= 1;
                    j -= 1;
                }
                Op::Delete => {
                    i -= 1;
                }
                Op::Insert => {
                    if !restriction.allows(text[j - 1]) {
                        return false;
                    }
                    j -= 1;
                }
                Op::Transpose => {
                    i -= 2;
                    j -= 2;
                }
            }
        }
        true
    }
}

/// Result of a fuzzy match.
#[derive(Debug, Clone)]
pub struct FuzzyMatchResult {
    /// End position of the match (byte offset, exclusive).
    pub end: usize,
    /// Similarity score (0.0 to 1.0).
    pub similarity: f32,
    /// Number of insertion edits.
    pub insertions: u8,
    /// Number of deletion edits.
    pub deletions: u8,
    /// Number of substitution edits.
    pub substitutions: u8,
    /// Number of transposition (swap) edits.
    pub swaps: u8,
}

impl FuzzyMatchResult {
    /// Returns the total number of edit operations.
    #[must_use]
    pub fn total_edits(&self) -> u8 {
        self.insertions
            .saturating_add(self.deletions)
            .saturating_add(self.substitutions)
            .saturating_add(self.swaps)
    }
}

/// Ordering for the cached candidate alignments of an embedded fuzzy literal.
///
/// `CachedMatches::get` returns the first entry, so this ranks the alignment
/// the matcher should use as its base for a fuzzy literal at a given position:
/// fewest edits first, then the longest span, then the highest similarity.
///
/// Edit count leads rather than similarity because the similarity score
/// saturates for short sub-matches — a 1-edit and a 2-edit match of a single
/// character both score 0.0 — so similarity alone cannot separate the minimal
/// alignment from an over-consuming one (e.g. matching `,` on `.12` as the
/// 1-edit `.`→`,` versus the 2-edit `.1`→`,`+insert). Feeding the matcher the
/// fewest-edit base lets its trailing-insertion logic re-derive the longer
/// variants, so the globally-minimal alignment survives. Longest-within-
/// fewest-edits preserves leftmost-longest for the base itself.
fn cmp_cached_candidates(a: &FuzzyMatchResult, b: &FuzzyMatchResult) -> std::cmp::Ordering {
    a.total_edits()
        .cmp(&b.total_edits())
        .then_with(|| b.end.cmp(&a.end))
        .then_with(|| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

#[test]
fn test_search_all_the_quick() {
    use crate::ir::LiteralPattern;
    use crate::types::FuzzyLimits;

    // Create a literal pattern for "quik" with 1 edit allowed
    let limits = FuzzyLimits::new().edits(1);
    let lit = LiteralPattern::new("quik".to_string(), Some(limits), None);

    let bridge = FuzzyBridge::new(&[lit], None, None, false).unwrap();

    let text = "The quick brown fox";
    let cached = bridge.search_all(text, 0.5);

    println!("search_all results for '{text}' with pattern 'quik~1':");
    println!("  by_pattern_and_start: {:?}", cached.by_pattern_and_start);

    assert!(
        !cached.by_pattern_and_start.is_empty(),
        "Should find at least one match"
    );
}
