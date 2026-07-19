//! Core matching engine using NFA simulation.

#![allow(
    clippy::needless_range_loop,
    clippy::match_same_arms,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::ref_option_ref,
    clippy::let_underscore_untyped,
    clippy::items_after_statements,
    clippy::float_cmp,
    clippy::allow_attributes,
    let_underscore_drop
)]

use std::sync::Arc;

use super::captures::CaptureState;
use super::fuzzy_bridge::{CachedMatches, FuzzyBridge, FuzzyMatchResult};
use crate::api::builder::{HandlerMap, HandlerResult};
use crate::ir::{LiteralPattern, Nfa, State, StateId};
use crate::parser::Anchor;
use crate::types::{Distance, FuzzyLimits, NumEdits};

/// A thread in the NFA simulation.
#[derive(Debug, Clone)]
struct Thread {
    /// Current NFA state.
    state: StateId,
    /// Current position in the text (byte index).
    pos: usize,
    /// Current match start position (can be reset by \K).
    match_start: usize,
    /// Capture state.
    captures: CaptureState,
    /// Accumulated similarity score.
    similarity: f32,
    /// Total edits.
    edits: EditCounts,
    /// Handler overrides: (`start_pos`, `end_pos`, `override_text`)
    handler_overrides: Vec<(usize, usize, String)>,
    /// Per fuzzy-group cumulative edit counts (indexed by `fuzzy_group_id`).
    group_edits: Vec<EditCounts>,
}

impl Default for Thread {
    fn default() -> Self {
        Thread {
            state: 0,
            pos: 0,
            match_start: 0,
            captures: CaptureState::new(0),
            similarity: 1.0,
            edits: EditCounts::default(),
            handler_overrides: Vec::new(),
            group_edits: Vec::new(),
        }
    }
}

/// Edit operation counts.
#[derive(Debug, Clone, Default)]
pub struct EditCounts {
    /// Number of character insertions.
    pub insertions: u8,
    /// Number of character deletions.
    pub deletions: u8,
    /// Number of character substitutions.
    pub substitutions: u8,
    /// Number of adjacent character swaps (transpositions).
    pub swaps: u8,
}

impl EditCounts {
    /// Get total edit count.
    #[must_use]
    pub fn total(&self) -> u8 {
        self.insertions
            .saturating_add(self.deletions)
            .saturating_add(self.substitutions)
            .saturating_add(self.swaps)
    }

    /// Calculate weighted cost of edits.
    /// Default cost is 1 for each edit type (equivalent to `total()`).
    #[must_use]
    pub fn cost(&self, i_cost: u8, d_cost: u8, s_cost: u8, t_cost: u8) -> u16 {
        u16::from(self.insertions) * u16::from(i_cost)
            + u16::from(self.deletions) * u16::from(d_cost)
            + u16::from(self.substitutions) * u16::from(s_cost)
            + u16::from(self.swaps) * u16::from(t_cost)
    }

    /// Merge with another edit count.
    #[must_use]
    pub fn merge(&self, other: &EditCounts) -> Self {
        EditCounts {
            insertions: self.insertions.saturating_add(other.insertions),
            deletions: self.deletions.saturating_add(other.deletions),
            substitutions: self.substitutions.saturating_add(other.substitutions),
            swaps: self.swaps.saturating_add(other.swaps),
        }
    }

    /// Create from a fuzzy match result.
    #[must_use]
    pub fn from_fuzzy_result(result: &FuzzyMatchResult) -> Self {
        EditCounts {
            insertions: result.insertions,
            deletions: result.deletions,
            substitutions: result.substitutions,
            swaps: result.swaps,
        }
    }
}

/// A match result.
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// Start position (byte index).
    pub start: usize,
    /// End position (byte index).
    pub end: usize,
    /// Similarity score (0.0 - 1.0).
    pub similarity: f32,
    /// Edit counts.
    pub edits: EditCounts,
    /// Capture groups.
    pub captures: CaptureState,
}

impl MatchResult {
    /// Get the matched text.
    #[must_use]
    pub fn as_str<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }
}

/// Check whether `match_edits` can be added to the group's cumulative edits
/// without exceeding the group budget.
fn check_group_budget(
    group_edits: &[EditCounts],
    group_id: Option<usize>,
    match_edits: &EditCounts,
    budgets: &[FuzzyLimits],
) -> bool {
    let Some(id) = group_id else { return true };
    if id >= budgets.len() || id >= group_edits.len() {
        return true;
    }
    let budget = &budgets[id];
    let merged = group_edits[id].merge(match_edits);
    if let Some(max_total) = budget.get_edits()
        && merged.total() > max_total
    {
        return false;
    }
    if let Some(max_ins) = budget.get_insertions()
        && merged.insertions > max_ins
    {
        return false;
    }
    if let Some(max_del) = budget.get_deletions()
        && merged.deletions > max_del
    {
        return false;
    }
    if let Some(max_sub) = budget.get_substitutions()
        && merged.substitutions > max_sub
    {
        return false;
    }
    if let Some(max_swap) = budget.get_swaps()
        && merged.swaps > max_swap
    {
        return false;
    }
    true
}

/// Merge `match_edits` into `group_edits` at the given `group_id`.
fn apply_group_edits(
    mut group_edits: Vec<EditCounts>,
    group_id: Option<usize>,
    match_edits: &EditCounts,
) -> Vec<EditCounts> {
    if let Some(id) = group_id
        && id < group_edits.len()
    {
        group_edits[id] = group_edits[id].merge(match_edits);
    }
    group_edits
}

/// Extract fuzzy group budgets from the NFA states.
/// Returns a vec indexed by `fuzzy_group_id`, mapping to the group's `FuzzyLimits`.
fn extract_fuzzy_group_budgets(nfa: &Nfa) -> Vec<FuzzyLimits> {
    use std::collections::BTreeMap;
    let mut budgets: BTreeMap<usize, FuzzyLimits> = BTreeMap::new();
    for state in &nfa.states {
        match state {
            State::FuzzyLiteral {
                fuzzy_group_id: Some(id),
                limits: Some(lim),
                ..
            }
            | State::FuzzyChar {
                fuzzy_group_id: Some(id),
                limits: Some(lim),
                ..
            } => {
                budgets.entry(*id).or_insert_with(|| lim.clone());
            }
            _ => {}
        }
    }
    budgets.into_values().collect()
}

/// Configuration for the matcher.
#[derive(Debug, Clone)]
pub struct MatcherConfig {
    /// Similarity threshold (0.0 - 1.0).
    pub threshold: f32,
    /// Maximum number of threads (beam width).
    pub max_threads: usize,
    /// Whether to search for matches anywhere (true) or only at start (false).
    pub unanchored: bool,
    /// BESTMATCH mode - find best match instead of first match.
    pub best_match: bool,
    /// ENHANCEMATCH mode - improve the fit of the found match.
    pub enhance_match: bool,
    /// POSIX mode - find longest match at leftmost position.
    pub posix: bool,
    /// Global mode - find all matches instead of stopping at first.
    /// When false (default), stops at first valid match (faster).
    pub global: bool,
    /// Multi-line mode - `^` and `$` match at line boundaries.
    pub multi_line: bool,
    /// Prefer shortest matches (for patterns with lazy quantifiers).
    /// When true, prefers shorter matches over longer ones at the same similarity.
    pub prefer_shortest: bool,
    /// Unicode mode - enable Unicode character classes (\w, \d, \s match Unicode).
    pub unicode: bool,
    /// Greedy first-match mode - return first match found (faster).
    /// Similar to mrab-regex behavior.
    pub greedy_first: bool,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        MatcherConfig {
            threshold: 0.8,
            max_threads: 1000,
            unanchored: true,
            best_match: false,
            enhance_match: false,
            posix: false,
            global: true, // Default: full NFA simulation for correctness
            multi_line: false,
            prefer_shortest: false,
            unicode: false,
            greedy_first: false,
        }
    }
}

/// The NFA-based matching engine.
pub struct Matcher<'a> {
    /// The NFA to execute.
    nfa: &'a Nfa,
    /// Bridge to fuzzy matching (Levenshtein automata).
    fuzzy_bridge: Option<&'a FuzzyBridge>,
    /// Number of capture groups.
    capture_count: usize,
    /// Configuration.
    config: MatcherConfig,
    /// Prefilter for fast candidate position detection (Arc for cheap cloning).
    prefilter: Arc<super::prefilter::Prefilter>,
    /// Whether this is a simple fuzzy-only pattern (can skip NFA simulation).
    is_simple_fuzzy: bool,
    /// Pattern indices for simple alternations (enables multi-pattern fast path).
    simple_alternation_indices: Option<Vec<usize>>,
    /// Multi-pattern prefilter for simple alternations.
    multi_prefilter: Option<super::prefilter::Prefilter>,
    /// First character class for quick rejection (when no prefilter is available).
    first_char_class: Option<crate::ir::hir::HirClass>,
    /// Whether the pattern ends with an End anchor ($).
    /// Enables reverse search optimization.
    ends_with_end_anchor: bool,
    /// Maximum match length for simple patterns (used with end anchor optimization).
    max_simple_length: Option<usize>,
    /// Custom handlers for (?call:name) patterns.
    handlers: &'a HandlerMap,
    /// Per-group shared edit budgets for multi-piece fuzzy groups.
    /// Indexed by `fuzzy_group_id` from the NFA states.
    fuzzy_group_budgets: Vec<FuzzyLimits>,
}

impl<'a> Matcher<'a> {
    /// Create a new matcher.
    #[must_use]
    pub fn new(
        nfa: &'a Nfa,
        fuzzy_bridge: Option<&'a FuzzyBridge>,
        capture_count: usize,
        config: MatcherConfig,
        handlers: &'a HandlerMap,
    ) -> Self {
        let is_simple_fuzzy =
            nfa.is_simple_fuzzy_only() && fuzzy_bridge.is_some_and(|b| b.pattern_count() == 1);
        let first_char_class = nfa.first_char_class();
        let ends_with_end_anchor = nfa.ends_with_end_anchor();
        let max_simple_length = Self::calculate_max_length(nfa, fuzzy_bridge);
        let fuzzy_group_budgets = extract_fuzzy_group_budgets(nfa);
        Matcher {
            nfa,
            fuzzy_bridge,
            capture_count,
            config,
            prefilter: Arc::new(super::prefilter::Prefilter::None),
            is_simple_fuzzy,
            simple_alternation_indices: None,
            multi_prefilter: None,
            first_char_class,
            ends_with_end_anchor,
            max_simple_length,
            handlers,
            fuzzy_group_budgets,
        }
    }

    /// Create a new matcher with a prefilter.
    #[must_use]
    pub fn with_prefilter(
        nfa: &'a Nfa,
        fuzzy_bridge: Option<&'a FuzzyBridge>,
        capture_count: usize,
        config: MatcherConfig,
        prefilter: Arc<super::prefilter::Prefilter>,
        handlers: &'a HandlerMap,
    ) -> Self {
        let is_simple_fuzzy =
            nfa.is_simple_fuzzy_only() && fuzzy_bridge.is_some_and(|b| b.pattern_count() == 1);

        // Detect simple alternations for multi-pattern fast path
        let (simple_alternation_indices, multi_prefilter) =
            Self::detect_simple_alternation(nfa, fuzzy_bridge);

        // Extract first char class for quick rejection (when no prefilter)
        let first_char_class = nfa.first_char_class();

        // Check if pattern ends with $ anchor
        let ends_with_end_anchor = nfa.ends_with_end_anchor();

        // Get max length for end-anchor optimization
        let max_simple_length = Self::calculate_max_length(nfa, fuzzy_bridge);

        let fuzzy_group_budgets = extract_fuzzy_group_budgets(nfa);
        Matcher {
            nfa,
            fuzzy_bridge,
            capture_count,
            config,
            prefilter,
            is_simple_fuzzy,
            simple_alternation_indices,
            multi_prefilter,
            first_char_class,
            ends_with_end_anchor,
            max_simple_length,
            handlers,
            fuzzy_group_budgets,
        }
    }

    /// Calculate the maximum match length using the bridge for `FuzzyLiteral` info.
    fn calculate_max_length(nfa: &Nfa, fuzzy_bridge: Option<&FuzzyBridge>) -> Option<usize> {
        // First try simple calculation (for patterns without FuzzyLiteral)
        if let Some(len) = nfa.max_simple_length() {
            return Some(len);
        }

        // Use length_range with bridge callback
        let (_, max_len) = nfa.length_range(|pattern_idx| {
            fuzzy_bridge.and_then(|b| {
                let char_len = b.pattern_char_len(pattern_idx)?;
                let max_edits = b.pattern_max_edits(pattern_idx).unwrap_or(0);
                Some((char_len, max_edits))
            })
        });

        max_len
    }

    /// Detect if the NFA is a simple alternation and build a multi-pattern prefilter.
    /// Returns None if any pattern has fuzzy edits (multi-pattern fast path doesn't support fuzzy yet).
    fn detect_simple_alternation(
        nfa: &Nfa,
        fuzzy_bridge: Option<&FuzzyBridge>,
    ) -> (Option<Vec<usize>>, Option<super::prefilter::Prefilter>) {
        let indices = nfa.get_alternation_pattern_indices();
        if indices.is_empty() {
            return (None, None);
        }

        let Some(bridge) = fuzzy_bridge else {
            return (None, None);
        };

        // Build multi-pattern prefilter
        let mut literals: Vec<(&str, u8)> = Vec::with_capacity(indices.len());
        for &idx in &indices {
            if let Some(text) = bridge.pattern_text(idx) {
                let max_edits = bridge.pattern_max_edits(idx).unwrap_or(0);
                literals.push((text, max_edits));
            }
        }

        if literals.is_empty() {
            return (None, None);
        }

        let multi_pf = super::prefilter::Prefilter::multi_fuzzy(&literals, false);

        // Return indices even if prefilter is inactive - allows fallback to individual streaming
        if multi_pf.is_active() {
            (Some(indices), Some(multi_pf))
        } else {
            (Some(indices), None)
        }
    }

    /// Find the first match in the text (or best match in BESTMATCH mode).
    #[must_use]
    pub fn find(&self, text: &str) -> Option<MatchResult> {
        // Multi-pattern fast path: for simple alternations (cat|bat|rat),
        // use parallel Bitap search instead of full NFA simulation.
        // Skip for POSIX mode which needs to find the longest match.
        let has_alt = self.simple_alternation_indices.is_some();
        let has_pf = self.multi_prefilter.is_some();
        if !self.config.global
            && !self.config.best_match
            && !self.config.enhance_match
            && !self.config.posix
            && self.config.unanchored
            && has_alt
            && has_pf
        {
            return self.find_multi_pattern_fast(text);
        }

        // Fallback for multi-pattern when prefilter is inactive (e.g., short patterns like 'a' with e<=1).
        // Use individual streaming Bitap search for each pattern.
        // Skip for POSIX mode which needs to find the longest match.
        if !self.config.global
            && !self.config.best_match
            && !self.config.enhance_match
            && !self.config.posix
            && self.config.unanchored
            && has_alt
            && !has_pf
        {
            return self.find_multi_pattern_individual_fallback(text);
        }

        // Non-global mode: search position by position, return on first match.
        // This is similar to mrab-regex behavior and much faster when matches exist early.
        // Also use this path when greedy_first is explicitly enabled.
        if !self.config.global
            && !self.config.best_match
            && !self.config.enhance_match
            && self.config.unanchored
            && (self.prefilter.is_active() || self.config.greedy_first)
            && self.fuzzy_bridge.is_some_and(|b| b.pattern_count() == 1)
        {
            return self.find_greedy_first(text);
        }

        // Streaming fallback for simple fuzzy patterns without prefilter (e.g., 'a' with e<=1).
        // Uses streaming Bitap which is O(N) instead of O(N*P) search_all.
        // Only used for simple fuzzy patterns where the literal IS the whole pattern.
        // Also use when greedy_first is enabled.
        if !self.config.global
            && !self.config.best_match
            && !self.config.enhance_match
            && self.config.unanchored
            && (!self.prefilter.is_active() || self.config.greedy_first)
            && self.is_simple_fuzzy
            && self.fuzzy_bridge.is_some_and(|b| b.pattern_count() == 1)
        {
            return self.find_streaming_fallback(text);
        }

        // Fast path for single-pattern first-match without prefilter.
        // Use search_first for early termination instead of search_all.
        // Skip if capture groups are needed (they require full search).
        if !self.config.global
            && !self.config.best_match
            && !self.config.enhance_match
            && self.config.unanchored
            && !self.prefilter.is_active()
            && self.fuzzy_bridge.is_some_and(|b| b.pattern_count() == 1)
            && self.capture_count == 0
        {
            // Use search_first for fast first-match
            if let Some(bridge) = self.fuzzy_bridge {
                if let Some(result) = bridge.search_first(text, self.config.threshold, 0) {
                    let mut captures = CaptureState::new(self.capture_count);
                    captures.set_full_match(result.start, result.end);
                    return Some(MatchResult {
                        start: result.start,
                        end: result.end,
                        similarity: result.similarity,
                        edits: EditCounts {
                            insertions: result.insertions,
                            deletions: result.deletions,
                            substitutions: result.substitutions,
                            swaps: result.swaps,
                        },
                        captures,
                    });
                }
                return None;
            }
        }

        // Fast path for anchored patterns: only check position 0.
        // For patterns without fuzzy literals, skip cache entirely.
        // For patterns with multiple fuzzy literals, we need full search because
        // different literals can match at different positions.
        if !self.config.unanchored && !self.config.best_match && !self.config.enhance_match {
            // Check if we can skip fuzzy search entirely
            let needs_fuzzy_cache = self.fuzzy_bridge.is_some_and(|b| !b.is_all_exact());

            if !needs_fuzzy_cache {
                // No fuzzy matching needed - just do NFA simulation at position 0
                return self.find_at_with_cache(text, 0, None);
            }

            // For single fuzzy literal, we can search only at position 0
            // For multiple literals, we need full search since they match at different positions
            let cached = self.fuzzy_bridge.map(|b| {
                if b.pattern_count() == 1 {
                    b.search_cached_at_position(text, 0, self.config.threshold)
                } else {
                    // Multiple patterns - need to search all positions
                    b.search_all(text, self.config.threshold)
                }
            });
            return self.find_at_with_cache(text, 0, cached.as_ref());
        }

        // Build fuzzy cache. For single-literal patterns, we can use the prefilter
        // to only search candidate positions. For multi-literal patterns, we must
        // search all positions since each literal can match at different offsets.
        let cached = self.fuzzy_bridge.map(|b| {
            if self.prefilter.is_active() && b.pattern_count() == 1 {
                b.search_all_with_prefilter(text, self.config.threshold, &self.prefilter)
            } else {
                b.search_all(text, self.config.threshold)
            }
        });

        if self.config.best_match || self.config.enhance_match {
            // BESTMATCH and ENHANCEMATCH both need a full search to minimize the
            // error count. The per-position NFA sim already keeps the lowest-cost
            // thread when enhance_match is set (see step dedup); scanning all start
            // positions here turns that into global error minimization instead of
            // returning the first position that happens to match.
            self.find_best_with_cache(text, cached.as_ref())
        } else if self.config.posix {
            // POSIX mode: find longest match at leftmost position
            self.find_posix_with_cache(text, cached.as_ref())
        } else if self.config.unanchored {
            // For first-match, use prefilter to skip impossible positions
            if self.prefilter.is_active() {
                let mut last_tried = None;
                let max_offset = self.prefilter.max_offset();
                for candidate in self.prefilter.find_candidates(text.as_bytes()) {
                    // Prefilter returns (found - max_offset), so we need to try positions
                    // from candidate to candidate + max_offset
                    for offset in 0..=max_offset {
                        let pos = candidate + offset;
                        if pos > text.len() {
                            continue;
                        }
                        // Ensure we're on a char boundary and haven't already tried this position
                        let idx = Self::snap_to_char_boundary(text, pos);
                        if last_tried == Some(idx) {
                            continue;
                        }
                        last_tried = Some(idx);

                        if let Some(m) = self.find_at_with_cache(text, idx, cached.as_ref()) {
                            return Some(m);
                        }
                    }
                }
                // Try at end for empty patterns
                self.find_at_with_cache(text, text.len(), cached.as_ref())
            } else if self.ends_with_end_anchor && !self.config.multi_line {
                // Pattern ends with $ - search from end (much faster for patterns like \.$)
                // (disabled in multiline mode where $ can match at any line boundary)
                if let Some(max_len) = self.max_simple_length {
                    // Only check last `max_len` character positions (very fast for short patterns)
                    // Iterate backwards from end, collecting only the positions we need
                    let bytes = text.as_bytes();
                    let mut positions = Vec::with_capacity(max_len + 1);
                    let mut byte_pos = bytes.len();
                    let mut chars_counted = 0;

                    while byte_pos > 0 && chars_counted < max_len {
                        byte_pos -= 1;
                        // Check if this is a UTF-8 start byte (not continuation byte 10xxxxxx)
                        if bytes[byte_pos] & 0b1100_0000 != 0b1000_0000 {
                            positions.push(byte_pos);
                            chars_counted += 1;
                        }
                    }

                    // Try positions from end (which is the start of positions since we collected backwards)
                    for &idx in &positions {
                        if let Some(ref fcc) = self.first_char_class {
                            let ch = text[idx..].chars().next().unwrap();
                            if !fcc.matches(ch) {
                                continue;
                            }
                        }
                        if let Some(m) = self.find_at_with_cache(text, idx, cached.as_ref()) {
                            return Some(m);
                        }
                    }
                    self.find_at_with_cache(text, text.len(), cached.as_ref())
                } else {
                    // Unknown max length - collect all positions and search in reverse
                    let positions: Vec<_> = text.char_indices().map(|(idx, _)| idx).collect();
                    for &idx in positions.iter().rev() {
                        if let Some(ref fcc) = self.first_char_class {
                            let ch = text[idx..].chars().next().unwrap();
                            if !fcc.matches(ch) {
                                continue;
                            }
                        }
                        if let Some(m) = self.find_at_with_cache(text, idx, cached.as_ref()) {
                            return Some(m);
                        }
                    }
                    self.find_at_with_cache(text, text.len(), cached.as_ref())
                }
            } else {
                // No prefilter - try every position, but use first_char_class for quick rejection
                for (idx, ch) in text.char_indices() {
                    // Quick reject: if first_char_class is set and doesn't match, skip
                    if let Some(ref fcc) = self.first_char_class
                        && !fcc.matches(ch)
                    {
                        continue;
                    }
                    if let Some(m) = self.find_at_with_cache(text, idx, cached.as_ref()) {
                        return Some(m);
                    }
                }
                self.find_at_with_cache(text, text.len(), cached.as_ref())
            }
        } else {
            self.find_at_with_cache(text, 0, cached.as_ref())
        }
    }

    /// Greedy first-match: search position by position, return on first match found.
    /// Much faster when matches exist early in text (similar to mrab-regex behavior).
    fn find_greedy_first(&self, text: &str) -> Option<MatchResult> {
        let bridge = self.fuzzy_bridge?;
        let max_offset = self.prefilter.max_offset();
        let text_bytes = text.as_bytes();

        // Try Guard NFA first - fastest path for single pattern with no prefilter needed
        if self.is_simple_fuzzy && !self.prefilter.is_active() {
            if let Some((start, result)) = bridge.find_first_guard_nfa(text, self.config.threshold)
            {
                let mut captures = CaptureState::new(self.capture_count);
                captures.set_full_match(start, result.end);
                return Some(MatchResult {
                    start,
                    end: result.end,
                    similarity: result.similarity,
                    edits: EditCounts {
                        insertions: result.insertions,
                        deletions: result.deletions,
                        substitutions: result.substitutions,
                        swaps: result.swaps,
                    },
                    captures,
                });
            }
            return None;
        }

        // Try lazy streaming search FIRST - best for early matches (like mrab-regex)
        // This processes candidates one at a time without collecting them all upfront
        let use_prefilter =
            self.prefilter.is_active() && self.prefilter.is_selective() && self.is_simple_fuzzy;
        if use_prefilter {
            // Use batch parallel for selective prefilters (few candidates), lazy otherwise
            if self.prefilter.selectivity() <= 2 {
                // Few candidates - use batch parallel with SIMD
                if let Some((start, result)) = bridge.find_first_batch_parallel(
                    text_bytes,
                    self.config.threshold,
                    &self.prefilter,
                ) {
                    let mut captures = CaptureState::new(self.capture_count);
                    captures.set_full_match(start, result.end);
                    return Some(MatchResult {
                        start,
                        end: result.end,
                        similarity: result.similarity,
                        edits: EditCounts::from_fuzzy_result(&result),
                        captures,
                    });
                }
            } else {
                // Many candidates - use lazy (better for early termination)
                if let Some((start, result)) =
                    bridge.find_first_lazy(text_bytes, self.config.threshold, &self.prefilter)
                {
                    let mut captures = CaptureState::new(self.capture_count);
                    captures.set_full_match(start, result.end);
                    return Some(MatchResult {
                        start,
                        end: result.end,
                        similarity: result.similarity,
                        edits: EditCounts::from_fuzzy_result(&result),
                        captures,
                    });
                }
            }
            return None;
        }

        // Fall back to streaming Bitap search (scans every character) - only when no prefilter
        if let Some((start, result)) =
            bridge.find_first_boyer_moore(text_bytes, self.config.threshold, max_offset)
        {
            // Fast path: for simple fuzzy-only patterns, skip NFA simulation entirely
            // The Bitap result already has all the information we need
            if self.is_simple_fuzzy {
                let mut captures = CaptureState::new(self.capture_count);
                captures.set_full_match(start, result.end);
                return Some(MatchResult {
                    start,
                    end: result.end,
                    similarity: result.similarity,
                    edits: EditCounts::from_fuzzy_result(&result),
                    captures,
                });
            }

            // Complex pattern: need NFA simulation
            let mut cached = CachedMatches::default();
            cached.insert(0, start, result);

            if let Some(m) = self.find_at_with_cache(text, start, Some(&cached)) {
                return Some(m);
            }
        }

        // Fall back to prefilter-based search for edge cases
        let mut last_tried = None;
        for candidate in self.prefilter.find_candidates(text_bytes) {
            for offset in 0..=max_offset {
                let pos = candidate + offset;
                if pos > text.len() {
                    continue;
                }

                let idx = Self::snap_to_char_boundary(text, pos);
                if last_tried == Some(idx) {
                    continue;
                }
                last_tried = Some(idx);

                // Fall back to slower path (e.g., for patterns longer than 64 chars)
                if let Some((start, result)) =
                    bridge.search_at_position(text, idx, self.config.threshold)
                {
                    // Fast path for simple patterns
                    if self.is_simple_fuzzy {
                        let mut captures = CaptureState::new(self.capture_count);
                        captures.set_full_match(start, result.end);
                        return Some(MatchResult {
                            start,
                            end: result.end,
                            similarity: result.similarity,
                            edits: EditCounts::from_fuzzy_result(&result),
                            captures,
                        });
                    }

                    let mut cached = CachedMatches::default();
                    cached.insert(0, start, result);

                    if let Some(m) = self.find_at_with_cache(text, start, Some(&cached)) {
                        return Some(m);
                    }
                }
            }
        }

        None
    }

    /// Multi-pattern fast path: for simple alternations like (cat|bat|rat),
    /// use parallel Bitap search instead of full NFA simulation.
    fn find_multi_pattern_fast(&self, text: &str) -> Option<MatchResult> {
        let bridge = self.fuzzy_bridge?;
        let indices = self.simple_alternation_indices.as_ref()?;
        let multi_prefilter = self.multi_prefilter.as_ref()?;

        let text_bytes = text.as_bytes();

        // Check if any pattern has fuzzy edits
        // Fuzzy patterns MUST use individual streaming because the prefilter-based
        // approach relies on exact byte matches which miss fuzzy variants
        let has_fuzzy_edits = indices
            .iter()
            .any(|&idx| bridge.pattern_max_edits(idx).unwrap_or(0) > 0);

        // Decide between combined prefilter vs individual pattern streaming.
        // Individual streaming is better when:
        // 1. Any pattern has fuzzy edits (prefilter can miss fuzzy variants), OR
        // 2. The prefilter is not selective AND patterns are long
        let use_individual = if has_fuzzy_edits {
            // Fuzzy patterns must use streaming - prefilter misses fuzzy variants
            true
        } else if multi_prefilter.is_selective() {
            false
        } else {
            // Check if patterns are long enough to benefit from individual streaming
            let min_pattern_len = indices
                .iter()
                .filter_map(|&idx| bridge.pattern_text(idx))
                .map(str::len)
                .min()
                .unwrap_or(0);
            // Long patterns (>= 8 chars) benefit from individual streaming
            min_pattern_len >= 8
        };

        let result = if use_individual {
            bridge.find_first_multi_pattern_individual(text_bytes, self.config.threshold, indices)
        } else {
            bridge.find_first_multi_pattern(
                text_bytes,
                self.config.threshold,
                indices,
                multi_prefilter,
            )
        };

        if let Some((_pattern_idx, start, result)) = result {
            let mut captures = CaptureState::new(self.capture_count);
            captures.set_full_match(start, result.end);
            return Some(MatchResult {
                start,
                end: result.end,
                similarity: result.similarity,
                edits: EditCounts::from_fuzzy_result(&result),
                captures,
            });
        }

        None
    }

    /// Fallback for multi-pattern search when prefilter is inactive.
    /// Uses individual streaming Bitap search for each pattern.
    fn find_multi_pattern_individual_fallback(&self, text: &str) -> Option<MatchResult> {
        let bridge = self.fuzzy_bridge?;
        let indices = self.simple_alternation_indices.as_ref()?;

        let text_bytes = text.as_bytes();

        // Use individual streaming search for each pattern
        if let Some((_pattern_idx, start, result)) =
            bridge.find_first_multi_pattern_individual(text_bytes, self.config.threshold, indices)
        {
            let mut captures = CaptureState::new(self.capture_count);
            captures.set_full_match(start, result.end);
            return Some(MatchResult {
                start,
                end: result.end,
                similarity: result.similarity,
                edits: EditCounts::from_fuzzy_result(&result),
                captures,
            });
        }

        None
    }

    /// Streaming fallback for single pattern without prefilter.
    /// Uses streaming Bitap which is O(N) instead of O(N*P) `search_all`.
    fn find_streaming_fallback(&self, text: &str) -> Option<MatchResult> {
        let bridge = self.fuzzy_bridge?;
        let text_bytes = text.as_bytes();

        // Use streaming Bitap for single pattern
        if let Some((start, result)) = bridge
            .find_first_multi_pattern_individual(text_bytes, self.config.threshold, &[0])
            .map(|(_, start, result)| (start, result))
        {
            let mut captures = CaptureState::new(self.capture_count);
            captures.set_full_match(start, result.end);
            return Some(MatchResult {
                start,
                end: result.end,
                similarity: result.similarity,
                edits: EditCounts::from_fuzzy_result(&result),
                captures,
            });
        }

        None
    }

    /// Snap a byte position to the nearest valid char boundary.
    #[inline]
    fn snap_to_char_boundary(text: &str, pos: usize) -> usize {
        if pos >= text.len() {
            return text.len();
        }
        // Find the start of the character at or after this position
        let bytes = text.as_bytes();
        let mut p = pos;
        while p < bytes.len() && (bytes[p] & 0b1100_0000) == 0b1000_0000 {
            p += 1;
        }
        p
    }

    /// Find (without caching, for debugging).
    ///
    /// This is useful for debugging and testing when you want to verify
    /// results without the caching optimization.
    #[cfg(test)]
    fn find_no_cache(&self, text: &str) -> Option<MatchResult> {
        if self.config.unanchored {
            for (idx, _) in text.char_indices() {
                if let Some(m) = self.find_at(text, idx) {
                    return Some(m);
                }
            }
            self.find_at(text, text.len())
        } else {
            self.find_at(text, 0)
        }
    }

    /// Find the best match in the text (BESTMATCH mode) using cached fuzzy matches.
    fn find_best_with_cache(
        &self,
        text: &str,
        cached: Option<&CachedMatches>,
    ) -> Option<MatchResult> {
        let mut best: Option<MatchResult> = None;

        // BESTMATCH comparison: lowest cost wins (following mrab-regex semantics)
        // Uses default cost of 1 for all edit types.
        // Tiebreakers: higher similarity, then longer match, then earlier start
        let is_better_match = |m: &MatchResult, current: &MatchResult| -> bool {
            // Primary: lower cost is better (cost(1,1,1,1) = total edits with equal weights)
            let m_cost = m.edits.cost(1, 1, 1, 1);
            let current_cost = current.edits.cost(1, 1, 1, 1);
            if m_cost != current_cost {
                return m_cost < current_cost;
            }

            // Secondary: higher similarity is better
            if (m.similarity - current.similarity).abs() > f32::EPSILON {
                return m.similarity > current.similarity;
            }

            // Tertiary: longer match is better
            let m_len = m.end - m.start;
            let current_len = current.end - current.start;
            if m_len != current_len {
                return m_len > current_len;
            }

            // Quaternary: earlier start is better
            m.start < current.start
        };

        // Try starting at each position
        for (idx, _) in text.char_indices() {
            if let Some(m) = self.find_at_with_cache(text, idx, cached) {
                // Perfect match (0 cost) - return immediately
                if m.edits.cost(1, 1, 1, 1) == 0 {
                    return Some(m);
                }

                let dominated = best
                    .as_ref()
                    .is_some_and(|current| !is_better_match(&m, current));
                if !dominated {
                    best = Some(m);
                }
            }
        }

        // Try at end for empty patterns
        if let Some(m) = self.find_at_with_cache(text, text.len(), cached) {
            if m.edits.total() == 0 {
                return Some(m);
            }

            let dominated = best
                .as_ref()
                .is_some_and(|current| !is_better_match(&m, current));
            if !dominated {
                best = Some(m);
            }
        }

        best
    }

    /// Find the longest match at the leftmost position (POSIX mode) using cached fuzzy matches.
    fn find_posix_with_cache(
        &self,
        text: &str,
        _cached: Option<&CachedMatches>,
    ) -> Option<MatchResult> {
        // POSIX: find leftmost-longest
        // We need overlapping matches, so we try each position and get the longest at each position

        let is_all_exact = self.fuzzy_bridge.is_none_or(FuzzyBridge::is_all_exact);
        let fuzzy_cached = if is_all_exact {
            None
        } else {
            self.fuzzy_bridge
                .map(|b| b.search_all(text, self.config.threshold))
        };

        let mut best: Option<MatchResult> = None;

        // Try each starting position (overlapping)
        for (idx, _) in text.char_indices() {
            if let Some(m) = self.find_at_with_cache(text, idx, fuzzy_cached.as_ref()) {
                if m.start != idx {
                    continue;
                }

                match &best {
                    None => best = Some(m),
                    Some(current) => {
                        if m.start < current.start
                            || (m.start == current.start && m.end > current.end)
                        {
                            best = Some(m);
                        }
                    }
                }
            }
        }

        best
    }

    /// Find all non-overlapping matches.
    #[must_use]
    pub fn find_all(&self, text: &str) -> Vec<MatchResult> {
        // For patterns where all literals are exact (no fuzzy edits), skip the cache entirely.
        // This avoids the overhead of building and querying the cache for common characters.
        let is_all_exact = self.fuzzy_bridge.is_none_or(FuzzyBridge::is_all_exact);

        let mut matches = Vec::new();

        // Optimization for end-anchored patterns: only check positions near the end
        // (disabled in multiline mode where $ can match at any line boundary)
        if self.ends_with_end_anchor
            && !self.config.multi_line
            && let Some(max_len) = self.max_simple_length
        {
            // Only check last `max_len` character positions
            let bytes = text.as_bytes();
            let mut positions = Vec::with_capacity(max_len + 1);
            let mut byte_pos = bytes.len();
            let mut chars_counted = 0;

            while byte_pos > 0 && chars_counted < max_len {
                byte_pos -= 1;
                if bytes[byte_pos] & 0b1100_0000 != 0b1000_0000 {
                    positions.push(byte_pos);
                    chars_counted += 1;
                }
            }

            // Build the fuzzy cache over ONLY the trailing window instead of the
            // O(N) full search_all: search the suffix `text[start_byte..]` and
            // shift positions back. bitap is local, so for matches ending at
            // text.len() (all that a `$` anchor accepts) the suffix yields
            // identical results to the full scan -- turning e.g.
            // `(?:here){e<=1}$` on a long haystack from O(N) into O(max_len).
            let start_byte = positions.last().copied().unwrap_or(0);
            let cached = if is_all_exact {
                None
            } else {
                self.fuzzy_bridge.map(|b| {
                    b.search_all(&text[start_byte..], self.config.threshold)
                        .shifted(start_byte)
                })
            };

            // Try positions from end (positions are already in reverse order)
            for &idx in &positions {
                if let Some(ref fcc) = self.first_char_class {
                    let ch = text[idx..].chars().next().unwrap();
                    if !fcc.matches(ch) {
                        continue;
                    }
                }
                if let Some(m) = self.find_at_with_cache(text, idx, cached.as_ref()) {
                    matches.push(m);
                    break; // End-anchored pattern can only match once
                }
            }

            // Try at end position for patterns that match empty string at end
            if matches.is_empty()
                && let Some(m) = self.find_at_with_cache(text, text.len(), cached.as_ref())
            {
                matches.push(m);
            }

            return matches;
        }

        // Non-end-anchored: search all fuzzy matches once upfront - O(N) instead
        // of O(N^2). Skip for exact patterns to avoid overhead.
        let cached = if is_all_exact {
            None
        } else {
            self.fuzzy_bridge
                .map(|b| b.search_all(text, self.config.threshold))
        };

        // Optimization: Use cached literal positions to guide search
        // If we have literal matches in the cache, only try positions within
        // MAX_LOOKBACK of a literal position
        if let Some(ref cache) = cached
            && !cache.is_empty()
        {
            return self.find_all_with_literal_guide(text, cache);
        }

        let mut pos = 0;

        while pos < text.len() {
            // Get the character at the current position for quick rejection
            let ch = text[pos..].chars().next();

            // Quick reject: if first_char_class is set and doesn't match, skip
            let should_try = match (&self.first_char_class, ch) {
                (Some(fcc), Some(c)) => fcc.matches(c),
                _ => true, // No first_char_class or at end, try anyway
            };

            if should_try {
                let result = self.find_at_with_cache(text, pos, cached.as_ref());
                if let Some(m) = result {
                    let end = m.end;
                    matches.push(m);
                    // Move past this match. For a forward (non-empty) match jump
                    // to its end; for a zero-width match fall through to the
                    // char-boundary-safe advance below (a raw `pos + 1` could
                    // land inside a multi-byte UTF-8 char and panic).
                    if end > pos {
                        pos = end;
                        continue;
                    }
                }
            }

            // Move to next character (char-boundary safe)
            pos = text[pos..]
                .char_indices()
                .nth(1)
                .map_or(text.len() + 1, |(i, _)| pos + i);
        }

        // Try at end for empty patterns (only if no first_char_class restriction)
        if self.first_char_class.is_none()
            && let Some(m) = self.find_at_with_cache(text, text.len(), cached.as_ref())
        {
            matches.push(m);
        }

        matches
    }

    /// Find up to `n` non-overlapping matches.
    ///
    /// This is more efficient than `find_all` when only a limited number of matches is needed,
    /// as it stops searching after finding `n` matches.
    #[must_use]
    pub fn find_n(&self, text: &str, n: usize) -> Vec<MatchResult> {
        if n == 0 {
            return Vec::new();
        }

        // For patterns where all literals are exact (no fuzzy edits), skip the cache entirely.
        let is_all_exact = self.fuzzy_bridge.is_none_or(FuzzyBridge::is_all_exact);

        // Search all fuzzy matches once upfront
        let cached = if is_all_exact {
            None
        } else {
            self.fuzzy_bridge
                .map(|b| b.search_all(text, self.config.threshold))
        };

        let mut matches = Vec::with_capacity(n);
        let mut pos = 0;

        while pos < text.len() && matches.len() < n {
            // Get the character at the current position for quick rejection
            let ch = text[pos..].chars().next();

            // Quick reject: if first_char_class is set and doesn't match, skip
            let should_try = match (&self.first_char_class, ch) {
                (Some(fcc), Some(c)) => fcc.matches(c),
                _ => true,
            };

            if should_try && let Some(m) = self.find_at_with_cache(text, pos, cached.as_ref()) {
                let end = m.end;
                matches.push(m);
                // For a forward match jump to its end; a zero-width match falls
                // through to the char-boundary-safe advance below (`pos + 1`
                // could land inside a multi-byte UTF-8 char and panic).
                if end > pos {
                    pos = end;
                    continue;
                }
            }

            // Move to next character (char-boundary safe)
            pos = text[pos..]
                .char_indices()
                .nth(1)
                .map_or(text.len() + 1, |(i, _)| pos + i);
        }

        matches
    }

    /// Find all matches using literal positions as a guide.
    /// Only tries positions that could reach a required literal.
    fn find_all_with_literal_guide(&self, text: &str, cached: &CachedMatches) -> Vec<MatchResult> {
        let mut matches = Vec::new();

        // Collect all literal positions sorted by start
        let mut literal_positions: Vec<usize> = cached
            .iter()
            .flat_map(|((_, start), _)| std::iter::once(start))
            .collect();
        literal_positions.sort_unstable();
        literal_positions.dedup();

        if literal_positions.is_empty() {
            return matches;
        }

        // Maximum lookback from a literal to where a match could start
        // This is a heuristic - for unbounded patterns, we use a large value
        const MAX_LOOKBACK: usize = 256;

        let mut pos = 0;
        let mut lit_idx = 0;

        while pos < text.len() && lit_idx < literal_positions.len() {
            let next_lit_pos = literal_positions[lit_idx];

            // Skip to position within MAX_LOOKBACK of the next literal
            if pos + MAX_LOOKBACK < next_lit_pos {
                // Jump to MAX_LOOKBACK before the literal
                pos = next_lit_pos.saturating_sub(MAX_LOOKBACK);
                // Snap to char boundary
                pos = Self::snap_to_char_boundary(text, pos);
            }

            // Get the character at the current position for quick rejection
            let ch = text[pos..].chars().next();

            // Quick reject: if first_char_class is set and doesn't match, skip
            let should_try = match (&self.first_char_class, ch) {
                (Some(fcc), Some(c)) => fcc.matches(c),
                _ => true,
            };

            if should_try && let Some(m) = self.find_at_with_cache(text, pos, Some(cached)) {
                let end = m.end;
                matches.push(m);
                // For a forward match jump to its end; a zero-width match falls
                // through to the char-boundary-safe advance below (`pos + 1`
                // could land inside a multi-byte UTF-8 char and panic).
                if end > pos {
                    pos = end;
                    // Advance lit_idx past positions we've covered
                    while lit_idx < literal_positions.len() && literal_positions[lit_idx] < pos {
                        lit_idx += 1;
                    }
                    continue;
                }
            }

            // Move to next character (char-boundary safe)
            let next_pos = text[pos..]
                .char_indices()
                .nth(1)
                .map_or(text.len() + 1, |(i, _)| pos + i);

            // If we've passed the current literal, move to the next one
            if next_pos > next_lit_pos {
                lit_idx += 1;
            }

            pos = next_pos;
        }

        matches
    }

    /// Try to find a match starting at a specific position.
    ///
    /// Unlike `find()` which searches the entire text, this starts the search
    /// at the given position. The full text is still used for boundary checks
    /// (e.g., `\b` word boundaries).
    #[must_use]
    pub fn find_at(&self, text: &str, start: usize) -> Option<MatchResult> {
        self.find_at_with_cache(text, start, None)
    }

    /// Build the fuzzy cache for the given text.
    ///
    /// This can be used for lazy iteration where we want to compute the cache
    /// once upfront and reuse it for multiple `find_at_with_cache` calls.
    pub fn build_cache(&self, text: &str) -> Option<CachedMatches> {
        // For patterns where all literals are exact (no fuzzy edits), skip the cache
        let is_all_exact = self.fuzzy_bridge.is_none_or(FuzzyBridge::is_all_exact);
        if is_all_exact {
            return None;
        }
        self.fuzzy_bridge
            .map(|b| b.search_all(text, self.config.threshold))
    }

    /// Try to find a match starting at a specific position using cached fuzzy matches.
    #[must_use]
    pub fn find_at_with_cache(
        &self,
        text: &str,
        start: usize,
        cached: Option<&CachedMatches>,
    ) -> Option<MatchResult> {
        use super::hash::FxHashSet;

        let group_count = self.fuzzy_group_budgets.len();
        let mut threads = vec![Thread {
            state: self.nfa.start,
            pos: start,
            match_start: start,
            captures: CaptureState::new(self.capture_count),
            similarity: 1.0,
            edits: EditCounts::default(),
            handler_overrides: Vec::new(),
            group_edits: vec![EditCounts::default(); group_count],
        }];

        let mut best_match: Option<MatchResult> = None;

        // Track visited (state, pos) pairs to avoid redundant exploration
        let mut visited: FxHashSet<(StateId, usize)> = FxHashSet::default();
        visited.insert((self.nfa.start, start));

        while !threads.is_empty() {
            // Early termination for lazy quantifiers: once we have a match, stop exploring
            if self.config.prefer_shortest && best_match.is_some() {
                break;
            }

            let mut next_threads = Vec::new();

            for thread in threads {
                self.step_thread_with_cache(
                    text,
                    start,
                    thread,
                    &mut next_threads,
                    &mut best_match,
                    cached,
                );
            }

            // Deduplicate threads by (state, pos)
            // In ENHANCEMATCH mode: keep the thread with LOWEST COST (fewest edits) at each position
            // In normal mode: keep the FIRST thread (for performance and correct "first match" semantics)
            let mut deduped = Vec::with_capacity(next_threads.len());
            if self.config.enhance_match {
                // ENHANCEMATCH: keep best thread at each (state, pos)
                // Best = lowest cost (using default cost of 1 for all edit types)
                use super::hash::FxHashMap;
                let mut best_at_pos: FxHashMap<(StateId, usize), usize> = FxHashMap::default();
                for (idx, thread) in next_threads.iter().enumerate() {
                    let key = (thread.state, thread.pos);
                    if let Some(&existing_idx) = best_at_pos.get(&key) {
                        // Keep the one with lower cost (fewer edits)
                        // Using cost(1,1,1,1) is equivalent to total() but more explicit
                        let new_cost = thread.edits.cost(1, 1, 1, 1);
                        let existing_cost = next_threads[existing_idx].edits.cost(1, 1, 1, 1);
                        if new_cost < existing_cost {
                            best_at_pos.insert(key, idx);
                        }
                    } else if visited.insert(key) {
                        best_at_pos.insert(key, idx);
                    }
                }
                // Collect the best threads
                let mut indices: Vec<_> = best_at_pos.values().copied().collect();
                indices.sort_unstable();
                for idx in indices {
                    deduped.push(next_threads[idx].clone());
                }
            } else {
                // Normal mode: keep first thread at each (state, pos)
                for thread in next_threads {
                    let key = (thread.state, thread.pos);
                    if visited.insert(key) {
                        deduped.push(thread);
                    }
                }
            }
            next_threads = deduped;

            // Prune threads if too many
            if next_threads.len() > self.config.max_threads {
                next_threads.sort_by(|a, b| {
                    b.similarity
                        .partial_cmp(&a.similarity)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                next_threads.truncate(self.config.max_threads);
            }

            threads = next_threads;
        }

        best_match
    }

    /// Process a single thread for one step, using cached fuzzy matches.
    fn step_thread_with_cache(
        &self,
        text: &str,
        _start: usize,
        thread: Thread,
        next_threads: &mut Vec<Thread>,
        best_match: &mut Option<MatchResult>,
        cached: Option<&CachedMatches>,
    ) {
        let state = &self.nfa.states[thread.state];

        match state {
            State::Accept => {
                let match_start = thread.match_start;
                let end_pos = thread.pos;
                let similarity = thread.similarity;
                let edits = thread.edits.clone();
                let overrides = thread.handler_overrides.clone();

                let mut captures = thread.captures.clone();
                captures.set_full_match(match_start, end_pos);
                captures.set_handler_overrides(overrides);

                let m = MatchResult {
                    start: match_start,
                    end: end_pos,
                    similarity,
                    edits,
                    captures,
                };

                // Keep best match:
                // - Earlier start position always wins
                // - At same start position:
                //   - In BESTMATCH/ENHANCEMATCH mode: prefer higher similarity, then longer
                //   - For simple alternations without fuzzy modifier: first match wins
                //   - For other patterns: prefer higher similarity, then longer (default behavior)
                let prefer_shorter = self.config.prefer_shortest;
                // Check if this is a simple alternation without any fuzzy edits
                let has_alternation = self.nfa.is_simple_alternation();
                // Check if any pattern in the alternation has fuzzy edits
                let has_fuzzy = self
                    .simple_alternation_indices
                    .as_ref()
                    .is_some_and(|indices| {
                        self.fuzzy_bridge.is_some_and(|bridge| {
                            indices
                                .iter()
                                .any(|&idx| bridge.pattern_max_edits(idx).unwrap_or(0) > 0)
                        })
                    });
                let is_simple_alt = has_alternation && !has_fuzzy;
                let is_special_mode = self.config.best_match || self.config.enhance_match;
                if best_match.as_ref().is_none_or(|best| {
                    // Earlier start wins
                    m.start < best.start
                        // At same start:
                        || (m.start == best.start && {
                        if is_special_mode {
                            // BESTMATCH/ENHANCEMATCH: prefer higher similarity/length
                            m.similarity > best.similarity
                                || (m.similarity == best.similarity && if prefer_shorter {
                                m.end - m.start < best.end - best.start
                            } else {
                                m.end - m.start > best.end - best.start
                            })
                        } else if is_simple_alt {
                            // Simple alternation (no fuzzy/captures): first match wins
                            false
                        } else {
                            // Default behavior: prefer higher similarity, then longer
                            m.similarity > best.similarity
                                || (m.similarity == best.similarity && if prefer_shorter {
                                m.end - m.start < best.end - best.start
                            } else {
                                m.end - m.start > best.end - best.start
                            })
                        }
                    })
                }) {
                    *best_match = Some(m);
                }
            }

            State::Epsilon { targets } => {
                for &target in targets {
                    next_threads.push(Thread {
                        state: target,
                        ..thread.clone()
                    });
                }
            }

            State::Split { branches, greedy } => {
                // For greedy: try branches in order (first branch is match/continue)
                // For non-greedy: try branches in reverse order (last branch is exit/skip)
                if *greedy {
                    for &branch in branches {
                        next_threads.push(Thread {
                            state: branch,
                            ..thread.clone()
                        });
                    }
                } else {
                    for &branch in branches.iter().rev() {
                        next_threads.push(Thread {
                            state: branch,
                            ..thread.clone()
                        });
                    }
                }
            }

            State::Char { class, next } => {
                if thread.pos < text.len()
                    && let Some(ch) = text[thread.pos..].chars().next()
                    && class.matches(ch)
                {
                    next_threads.push(Thread {
                        state: *next,
                        pos: thread.pos + ch.len_utf8(),
                        ..thread
                    });
                }
            }

            State::FuzzyChar {
                class,
                limits,
                min_edits: _,
                cost_constraint: _,
                fuzzy_group_id,
                next,
            } => {
                // Calculate edit budget
                let max_edits = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_edits)
                    .unwrap_or(u8::MAX);
                let max_deletions = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_deletions)
                    .unwrap_or(max_edits);
                let max_substitutions = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_substitutions)
                    .unwrap_or(max_edits);
                let max_insertions = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_insertions)
                    .unwrap_or(max_edits);

                let current_edits = thread.edits.total();

                // Calculate similarity penalty per edit
                let edit_penalty = if max_edits > 0 {
                    1.0 / (f32::from(max_edits) + 1.0)
                } else {
                    1.0
                };

                // Emit a completed class-char match advancing to `next` at
                // `base_pos`, then also emit "trailing insertion" variants that
                // consume following text chars as insertions (within budget).
                // Combined with the leading-insertion self-loop below, this lets
                // a fuzzy class match extra characters on either side, e.g.
                // `^(?:[cd]){i<=1}$` matching "dz" (trailing) or "zd" (leading).
                let push_match_and_trailing =
                    |next_threads: &mut Vec<Thread>,
                     base_pos: usize,
                     base_edits: EditCounts,
                     base_sim: f32| {
                        let base_delta = EditCounts {
                            insertions: base_edits.insertions - thread.edits.insertions,
                            deletions: base_edits.deletions - thread.edits.deletions,
                            substitutions: base_edits.substitutions - thread.edits.substitutions,
                            swaps: base_edits.swaps - thread.edits.swaps,
                        };
                        if check_group_budget(
                            &thread.group_edits,
                            *fuzzy_group_id,
                            &base_delta,
                            &self.fuzzy_group_budgets,
                        ) {
                            next_threads.push(Thread {
                                state: *next,
                                pos: base_pos,
                                similarity: base_sim,
                                edits: base_edits.clone(),
                                captures: thread.captures.clone(),
                                match_start: thread.match_start,
                                handler_overrides: thread.handler_overrides.clone(),
                                group_edits: apply_group_edits(
                                    thread.group_edits.clone(),
                                    *fuzzy_group_id,
                                    &base_delta,
                                ),
                            });
                        }
                        let mut tpos = base_pos;
                        let mut edits = base_edits;
                        let mut extra_insertions: u8 = 0;
                        let mut sim = base_sim;
                        while edits.total() < max_edits
                            && edits.insertions < max_insertions
                            && tpos < text.len()
                        {
                            let Some(tch) = text[tpos..].chars().next() else {
                                break;
                            };
                            extra_insertions += 1;
                            let acc_delta = EditCounts {
                                insertions: base_delta.insertions + extra_insertions,
                                deletions: base_delta.deletions,
                                substitutions: base_delta.substitutions,
                                swaps: base_delta.swaps,
                            };
                            if !check_group_budget(
                                &thread.group_edits,
                                *fuzzy_group_id,
                                &acc_delta,
                                &self.fuzzy_group_budgets,
                            ) {
                                break;
                            }
                            tpos += tch.len_utf8();
                            edits.insertions += 1;
                            sim *= 1.0 - edit_penalty;
                            next_threads.push(Thread {
                                state: *next,
                                pos: tpos,
                                similarity: sim,
                                edits: edits.clone(),
                                captures: thread.captures.clone(),
                                match_start: thread.match_start,
                                handler_overrides: thread.handler_overrides.clone(),
                                group_edits: apply_group_edits(
                                    thread.group_edits.clone(),
                                    *fuzzy_group_id,
                                    &acc_delta,
                                ),
                            });
                        }
                    };

                // 1. Try exact match or substitution
                if thread.pos < text.len()
                    && let Some(ch) = text[thread.pos..].chars().next()
                {
                    if class.matches(ch) {
                        // Exact match - advance both pattern and text
                        push_match_and_trailing(
                            next_threads,
                            thread.pos + ch.len_utf8(),
                            thread.edits.clone(),
                            thread.similarity,
                        );
                    } else if current_edits < max_edits
                        && thread.edits.substitutions < max_substitutions
                    {
                        // Check group budget for the substitution
                        let sub_edit = EditCounts {
                            substitutions: 1,
                            ..EditCounts::default()
                        };
                        if check_group_budget(
                            &thread.group_edits,
                            *fuzzy_group_id,
                            &sub_edit,
                            &self.fuzzy_group_budgets,
                        ) {
                            // Substitution - consume mismatched char
                            let mut new_edits = thread.edits.clone();
                            new_edits.substitutions += 1;
                            push_match_and_trailing(
                                next_threads,
                                thread.pos + ch.len_utf8(),
                                new_edits,
                                thread.similarity * (1.0 - edit_penalty),
                            );
                        }
                    }
                }

                // 2. Try deletion (skip this pattern char without consuming text)
                if current_edits < max_edits && thread.edits.deletions < max_deletions {
                    let del_edit = EditCounts {
                        deletions: 1,
                        ..EditCounts::default()
                    };
                    if check_group_budget(
                        &thread.group_edits,
                        *fuzzy_group_id,
                        &del_edit,
                        &self.fuzzy_group_budgets,
                    ) {
                        let mut new_edits = thread.edits.clone();
                        new_edits.deletions += 1;
                        next_threads.push(Thread {
                            state: *next,
                            pos: thread.pos,
                            similarity: thread.similarity * (1.0 - edit_penalty),
                            edits: new_edits,
                            group_edits: apply_group_edits(
                                thread.group_edits.clone(),
                                *fuzzy_group_id,
                                &del_edit,
                            ),
                            ..thread.clone()
                        });
                    }
                }

                // 3. Try leading insertion (consume an extra text char before the
                // class char). Self-loop on this FuzzyChar state; the (state, pos)
                // dedup in the driver keeps it terminating.
                if thread.pos < text.len()
                    && current_edits < max_edits
                    && thread.edits.insertions < max_insertions
                    && let Some(ch) = text[thread.pos..].chars().next()
                {
                    let ins_edit = EditCounts {
                        insertions: 1,
                        ..EditCounts::default()
                    };
                    if check_group_budget(
                        &thread.group_edits,
                        *fuzzy_group_id,
                        &ins_edit,
                        &self.fuzzy_group_budgets,
                    ) {
                        let mut new_edits = thread.edits.clone();
                        new_edits.insertions += 1;
                        next_threads.push(Thread {
                            state: thread.state,
                            pos: thread.pos + ch.len_utf8(),
                            similarity: thread.similarity * (1.0 - edit_penalty),
                            edits: new_edits,
                            captures: thread.captures.clone(),
                            match_start: thread.match_start,
                            handler_overrides: thread.handler_overrides.clone(),
                            group_edits: apply_group_edits(
                                thread.group_edits.clone(),
                                *fuzzy_group_id,
                                &ins_edit,
                            ),
                        });
                    }
                }
            }

            State::FuzzyLiteral {
                pattern_index,
                next,
                limits,
                min_edits,
                cost_constraint,
                fuzzy_group_id,
            } => {
                if let Some(bridge) = self.fuzzy_bridge {
                    // Fast path for exact matching (no fuzzy edits)
                    // This avoids the full bridge lookup overhead for patterns without edits
                    let max_edits = limits.as_ref().map(|l| {
                        l.get_edits().unwrap_or_else(|| {
                            let i = l.get_insertions().unwrap_or(0);
                            let d = l.get_deletions().unwrap_or(0);
                            let s = l.get_substitutions().unwrap_or(0);
                            let t = l.get_swaps().unwrap_or(0);
                            i.saturating_add(d).saturating_add(s).saturating_add(t)
                        })
                    });

                    if max_edits.is_none() || max_edits == Some(0) {
                        // Exact match fast path - direct string comparison
                        if let Some(pattern_text) = bridge.pattern_text(*pattern_index)
                            && thread.pos + pattern_text.len() <= text.len()
                            && text[thread.pos..].starts_with(pattern_text)
                        {
                            let new_end = thread.pos + pattern_text.len();
                            // Only accept if we consume at least one character
                            if new_end > thread.pos
                                && check_group_budget(
                                    &thread.group_edits,
                                    *fuzzy_group_id,
                                    &EditCounts::default(),
                                    &self.fuzzy_group_budgets,
                                )
                            {
                                next_threads.push(Thread {
                                    state: *next,
                                    pos: new_end,
                                    similarity: thread.similarity,
                                    edits: thread.edits.clone(),
                                    captures: thread.captures.clone(),
                                    group_edits: thread.group_edits.clone(),
                                    ..thread
                                });
                            }
                        }
                        // Skip the full fuzzy matching path for exact patterns
                    } else {
                        let expected_end = self.find_expected_end(*next, text.len());

                        let meets_min_edits = |result: &FuzzyMatchResult| -> bool {
                            match min_edits {
                                Some(min) => result.total_edits() >= *min,
                                None => true,
                            }
                        };

                        let meets_cost_constraint = |result: &FuzzyMatchResult| -> bool {
                            match cost_constraint {
                                Some(constraint) => constraint.is_satisfied(
                                    result.insertions,
                                    result.deletions,
                                    result.substitutions,
                                    result.swaps,
                                ),
                                None => true,
                            }
                        };

                        let meets_all_constraints = |result: &FuzzyMatchResult| -> bool {
                            meets_min_edits(result) && meets_cost_constraint(result)
                        };

                        // Use cached results if available, otherwise fall back to direct search
                        let result = if let Some(cache) = cached {
                            bridge.find_at_cached(cache, *pattern_index, thread.pos)
                        } else {
                            bridge.find_at(text, *pattern_index, thread.pos, self.config.threshold)
                        };

                        if let Some(result) = result {
                            let should_try_boundary = expected_end.is_some()
                                && result.end < expected_end.unwrap()
                                && limits.as_ref().is_some_and(|l| {
                                    l.get_insertions().unwrap_or(0) > 0
                                        || l.get_edits().unwrap_or(0) > 0
                                });

                            if should_try_boundary
                                && let Some(boundary_result) = bridge.find_with_boundary_insertions(
                                    text,
                                    *pattern_index,
                                    thread.pos,
                                    expected_end,
                                    self.config.threshold,
                                    cached,
                                )
                                && meets_all_constraints(&boundary_result)
                            {
                                let match_edits = EditCounts::from_fuzzy_result(&boundary_result);
                                if check_group_budget(
                                    &thread.group_edits,
                                    *fuzzy_group_id,
                                    &match_edits,
                                    &self.fuzzy_group_budgets,
                                ) {
                                    next_threads.push(Thread {
                                        state: *next,
                                        pos: boundary_result.end,
                                        similarity: thread.similarity * boundary_result.similarity,
                                        edits: thread.edits.merge(&match_edits),
                                        captures: thread.captures.clone(),
                                        match_start: thread.match_start,
                                        handler_overrides: thread.handler_overrides.clone(),
                                        group_edits: apply_group_edits(
                                            thread.group_edits.clone(),
                                            *fuzzy_group_id,
                                            &match_edits,
                                        ),
                                    });
                                }
                            }

                            // Only accept matches that consume at least one character
                            // to prevent infinite loops with quantifiers when fuzzy
                            // match can delete entire pattern
                            if meets_all_constraints(&result) && result.end > thread.pos {
                                let match_edits = EditCounts::from_fuzzy_result(&result);
                                if check_group_budget(
                                    &thread.group_edits,
                                    *fuzzy_group_id,
                                    &match_edits,
                                    &self.fuzzy_group_budgets,
                                ) {
                                    next_threads.push(Thread {
                                        state: *next,
                                        pos: result.end,
                                        similarity: thread.similarity * result.similarity,
                                        edits: thread.edits.merge(&match_edits),
                                        captures: thread.captures.clone(),
                                        match_start: thread.match_start,
                                        handler_overrides: thread.handler_overrides.clone(),
                                        group_edits: apply_group_edits(
                                            thread.group_edits.clone(),
                                            *fuzzy_group_id,
                                            &match_edits,
                                        ),
                                    });
                                }
                            }
                        } else {
                            let can_use_boundary = limits.as_ref().is_some_and(|l| {
                                l.get_insertions().unwrap_or(0) > 0
                                    || l.get_edits().unwrap_or(0) > 0
                            });

                            if can_use_boundary
                                && let Some(result) = bridge.find_with_boundary_insertions(
                                    text,
                                    *pattern_index,
                                    thread.pos,
                                    expected_end,
                                    self.config.threshold,
                                    cached,
                                )
                            {
                                // Must consume at least one character
                                if meets_all_constraints(&result) && result.end > thread.pos {
                                    let match_edits = EditCounts::from_fuzzy_result(&result);
                                    if check_group_budget(
                                        &thread.group_edits,
                                        *fuzzy_group_id,
                                        &match_edits,
                                        &self.fuzzy_group_budgets,
                                    ) {
                                        next_threads.push(Thread {
                                            state: *next,
                                            pos: result.end,
                                            similarity: thread.similarity * result.similarity,
                                            edits: thread.edits.merge(&match_edits),
                                            captures: thread.captures.clone(),
                                            match_start: thread.match_start,
                                            handler_overrides: thread.handler_overrides.clone(),
                                            group_edits: apply_group_edits(
                                                thread.group_edits.clone(),
                                                *fuzzy_group_id,
                                                &match_edits,
                                            ),
                                        });
                                    }
                                }
                            }
                        }

                        // Also try pure deletion path: skip entire pattern without consuming text
                        // This allows patterns like (?:b){e<=1}(?:c){e<=1} to match "c"
                        // by deleting 'b' and exactly matching 'c'
                        // Only do this when:
                        // 1. We've already consumed some text (thread.pos > 0) - prevents spurious empty matches at start
                        // 2. AND there's still text remaining (thread.pos < text.len()) - at end, find_at handles deletion
                        let max_edits_for_deletion = max_edits.unwrap_or(0);
                        let max_deletions = limits
                            .as_ref()
                            .and_then(FuzzyLimits::get_deletions)
                            .unwrap_or(max_edits_for_deletion);

                        // Only allow pure deletion when we're in the middle of matching (not at start)
                        // and there's still text to match by subsequent patterns
                        if max_deletions > 0 && thread.pos > 0 && thread.pos < text.len() {
                            // Get pattern length to determine deletion cost
                            if let Some(pattern_char_len) = bridge.pattern_char_len(*pattern_index)
                            {
                                let pattern_len = pattern_char_len as u8;
                                let current_deletions = thread.edits.deletions;

                                // Can we delete the entire pattern?
                                if current_deletions + pattern_len <= max_deletions
                                    && (thread.edits.total() as usize + pattern_len as usize)
                                        <= max_edits_for_deletion as usize
                                {
                                    // Calculate similarity for pure deletion using same formula as Bitap:
                                    // similarity = 1 - total_edits / max_len
                                    // For pure deletion, match_len=0, so max_len = pattern_len
                                    let similarity = (1.0
                                        - f32::from(pattern_len) / f32::from(pattern_len))
                                    .max(0.0);

                                    // Check min_edits constraint
                                    let deletion_result = FuzzyMatchResult {
                                        end: thread.pos,
                                        similarity,
                                        insertions: 0,
                                        deletions: pattern_len,
                                        substitutions: 0,
                                        swaps: 0,
                                    };

                                    if meets_all_constraints(&deletion_result) {
                                        let match_edits = EditCounts {
                                            insertions: 0,
                                            deletions: pattern_len,
                                            substitutions: 0,
                                            swaps: 0,
                                        };
                                        if check_group_budget(
                                            &thread.group_edits,
                                            *fuzzy_group_id,
                                            &match_edits,
                                            &self.fuzzy_group_budgets,
                                        ) {
                                            let mut new_edits = thread.edits.clone();
                                            new_edits.deletions += pattern_len;
                                            next_threads.push(Thread {
                                                state: *next,
                                                pos: thread.pos,
                                                similarity: thread.similarity
                                                    * deletion_result.similarity,
                                                edits: new_edits,
                                                captures: thread.captures.clone(),
                                                match_start: thread.match_start,
                                                handler_overrides: thread.handler_overrides.clone(),
                                                group_edits: apply_group_edits(
                                                    thread.group_edits.clone(),
                                                    *fuzzy_group_id,
                                                    &match_edits,
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    } // Close the else block for fuzzy matching
                }
            }

            State::CaptureStart { index, next } => {
                let mut captures = thread.captures.clone();
                captures.start_capture(*index, thread.pos);
                next_threads.push(Thread {
                    state: *next,
                    captures,
                    ..thread
                });
            }

            State::CaptureEnd { index, next } => {
                let mut captures = thread.captures.clone();
                captures.end_capture(*index, thread.pos);
                next_threads.push(Thread {
                    state: *next,
                    captures,
                    ..thread
                });
            }

            State::Anchor { kind, next } => {
                let matches = match kind {
                    Anchor::Start => {
                        if self.config.multi_line {
                            Self::is_line_start(text, thread.pos)
                        } else {
                            thread.pos == 0
                        }
                    }
                    Anchor::End => {
                        if self.config.multi_line {
                            Self::is_line_end(text, thread.pos)
                        } else {
                            thread.pos == text.len()
                        }
                    }
                    Anchor::WordBoundary => Self::is_word_boundary(text, thread.pos),
                    Anchor::NotWordBoundary => !Self::is_word_boundary(text, thread.pos),
                };

                if matches {
                    next_threads.push(Thread {
                        state: *next,
                        ..thread
                    });
                }
            }

            State::Lookahead {
                positive,
                nfa,
                literals,
                next,
            } => {
                // Build a fuzzy bridge for the lookahead from its literals
                let lookahead_bridge = if literals.is_empty() {
                    None
                } else {
                    FuzzyBridge::new(literals, None, None, false)
                };

                // Create a sub-matcher for the lookahead with its own bridge
                let sub_matcher = Matcher::new(
                    nfa,
                    lookahead_bridge.as_ref(),
                    0,
                    MatcherConfig {
                        unanchored: false,
                        ..self.config.clone()
                    },
                    self.handlers,
                );

                let sub_result = sub_matcher.find_at(text, thread.pos);

                let has_match = sub_result.is_some();

                if has_match == *positive {
                    next_threads.push(Thread {
                        state: *next,
                        ..thread
                    });
                }
            }

            State::LookaheadLiteral {
                positive,
                literal,
                next,
            } => {
                // Inline literal lookahead - check if literal exists at current position
                let bytes = text.as_bytes();
                let pos = thread.pos;
                let matched = pos + literal.len() <= bytes.len()
                    && &bytes[pos..pos + literal.len()] == literal.as_slice();
                if matched == *positive {
                    next_threads.push(Thread {
                        state: *next,
                        ..thread
                    });
                }
            }

            State::Lookbehind {
                positive,
                nfa,
                literals,
                bridge,
                next,
            } => {
                let has_match =
                    self.try_lookbehind(text, thread.pos, nfa, literals, bridge.as_deref());

                if has_match == *positive {
                    next_threads.push(Thread {
                        state: *next,
                        ..thread
                    });
                }
            }

            State::LookbehindLiteral {
                positive,
                literal,
                next,
            } => {
                // Inline literal lookbehind - check if literal exists before current position
                let bytes = text.as_bytes();
                let pos = thread.pos;
                let matched =
                    pos >= literal.len() && &bytes[pos - literal.len()..pos] == literal.as_slice();
                if matched == *positive {
                    next_threads.push(Thread {
                        state: *next,
                        ..thread
                    });
                }
            }

            State::Backreference {
                group,
                next,
                limits,
            } => {
                if let Some((cap_start, cap_end)) = thread.captures.get(*group) {
                    let captured = &text[cap_start..cap_end];
                    let remaining = &text[thread.pos..];

                    if let Some(limits) = limits {
                        let max_edits = limits.get_edits().unwrap_or(
                            limits.get_insertions().unwrap_or(0)
                                + limits.get_deletions().unwrap_or(0)
                                + limits.get_substitutions().unwrap_or(0),
                        );

                        let cap_len = captured.len();
                        let min_len = cap_len.saturating_sub(max_edits as usize);
                        let max_len = cap_len.saturating_add(max_edits as usize);

                        for end_len in min_len..=max_len.min(remaining.len()) {
                            if !remaining.is_char_boundary(end_len) {
                                continue;
                            }
                            let candidate = &remaining[..end_len];
                            let distance = edit_distance_bounded(captured, candidate, max_edits);

                            if distance != Distance::MAX && distance <= max_edits.into() {
                                next_threads.push(Thread {
                                    state: *next,
                                    pos: thread.pos + end_len,
                                    similarity: thread.similarity
                                        * (1.0
                                            - distance as f32 / cap_len.max(end_len).max(1) as f32),
                                    edits: thread.edits.merge(&EditCounts {
                                        substitutions: distance as u8,
                                        ..Default::default()
                                    }),
                                    captures: thread.captures.clone(),
                                    match_start: thread.match_start,
                                    handler_overrides: thread.handler_overrides.clone(),
                                    group_edits: Vec::new(),
                                });
                            }
                        }
                    } else if remaining.starts_with(captured) {
                        next_threads.push(Thread {
                            state: *next,
                            pos: thread.pos + captured.len(),
                            match_start: thread.match_start,
                            captures: thread.captures.clone(),
                            similarity: thread.similarity,
                            edits: thread.edits.clone(),
                            handler_overrides: thread.handler_overrides.clone(),
                            group_edits: thread.group_edits.clone(),
                        });
                    }
                }
            }

            // New NFA states - \K resets match start
            State::ResetMatchStart { next } => {
                let mut new_thread = thread.clone();
                new_thread.match_start = thread.pos;
                new_thread.state = *next;
                next_threads.push(new_thread);
            }

            // Custom handler invocation
            State::Handler { name, next } => {
                // Look up the handler
                if let Some(handler) = self.handlers.get(name) {
                    // Call the handler with current text and position
                    match handler(text, thread.pos) {
                        HandlerResult::MatchOverride(consumed, override_text) => {
                            // Handler matched - advance position and record override
                            let mut new_thread = thread.clone();
                            new_thread.pos += consumed;
                            new_thread.state = *next;
                            new_thread.handler_overrides.push((
                                thread.pos,
                                thread.pos + consumed,
                                override_text,
                            ));
                            next_threads.push(new_thread);
                        }
                        HandlerResult::NoMatch => {
                            // Handler didn't match - thread dies
                        }
                    }
                } else {
                    // Handler not found - treat as error (thread dies)
                }
            }
            State::AtomicGroup { .. }
            | State::RecursivePattern { .. }
            | State::RecursiveGroup { .. }
            | State::RecursiveNamedGroup { .. } => {}
        }
    }

    /// Check if position is at a word boundary.
    /// Optimized to avoid O(n) scan - walks backwards at most 4 bytes (max UTF-8 char length).
    #[inline]
    fn is_word_boundary(text: &str, pos: usize) -> bool {
        let bytes = text.as_bytes();

        // Get character before pos (walk backwards to find UTF-8 char start)
        let before_is_word = if pos > 0 {
            // UTF-8 continuation bytes have pattern 10xxxxxx (0x80-0xBF)
            // Walk back at most 4 bytes to find the leading byte
            let mut start = pos - 1;
            while start > 0 && (bytes[start] & 0xC0) == 0x80 {
                start -= 1;
            }
            // Decode the single character
            text[start..pos]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        } else {
            false
        };

        // Get character at pos
        let after_is_word = text[pos..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');

        before_is_word != after_is_word
    }

    /// Check if position is at the start of a line (after newline or at string start).
    #[inline]
    fn is_line_start(text: &str, pos: usize) -> bool {
        if pos == 0 {
            return true;
        }
        // Check if the character before pos is a newline
        let bytes = text.as_bytes();
        bytes[pos - 1] == b'\n'
    }

    /// Check if position is at the end of a line (before newline or at string end).
    #[inline]
    fn is_line_end(text: &str, pos: usize) -> bool {
        if pos == text.len() {
            return true;
        }
        // Check if the character at pos is a newline
        let bytes = text.as_bytes();
        bytes[pos] == b'\n'
    }

    /// Check if the path from a state leads to an End anchor (through non-consuming states).
    /// Returns `Some(text_len)` if we should expect the match to extend to end of text.
    fn find_expected_end(&self, state_id: StateId, text_len: usize) -> Option<usize> {
        let mut visited = vec![false; self.nfa.states.len()];
        self.find_expected_end_recursive(state_id, text_len, &mut visited)
    }

    /// Recursive helper for `find_expected_end`.
    fn find_expected_end_recursive(
        &self,
        state_id: StateId,
        text_len: usize,
        visited: &mut Vec<bool>,
    ) -> Option<usize> {
        if visited[state_id] {
            return None;
        }
        visited[state_id] = true;

        match &self.nfa.states[state_id] {
            // Found End anchor - return the expected end position
            State::Anchor {
                kind: Anchor::End,
                ..
            } => Some(text_len),

            State::CaptureEnd { next, .. } => {
                self.find_expected_end_recursive(*next, text_len, visited)
            }

            // Other anchors (like word boundary) - continue through
            State::Anchor { next, .. } => self.find_expected_end_recursive(*next, text_len, visited),

            // Lookarounds don't consume characters - continue through
            State::Lookahead { next, .. }
            | State::LookaheadLiteral { next, .. }
            | State::Lookbehind { next, .. }
            | State::LookbehindLiteral { next, .. } => {
                self.find_expected_end_recursive(*next, text_len, visited)
            }

            // Accept without End anchor - no expected end
            State::Accept
            | State::Epsilon { .. }
            | State::Split { .. }
            // Consuming states - stop searching
            | State::Char { .. }
            | State::FuzzyChar { .. }
            | State::FuzzyLiteral { .. }
            | State::Backreference { .. }
            | State::CaptureStart { .. }
            | State::ResetMatchStart { .. }
            | State::AtomicGroup { .. }
            | State::RecursivePattern { .. }
            | State::RecursiveGroup { .. }
            | State::RecursiveNamedGroup { .. }
            | State::Handler { .. } => None,
        }
    }

    /// Try to match a lookbehind pattern.
    ///
    /// Uses pattern length calculation to efficiently search only valid positions.
    /// `pos` is a byte index in the text.
    /// `bridge` is the pre-built `FuzzyBridge` for the lookbehind's literals.
    fn try_lookbehind(
        &self,
        text: &str,
        pos: usize,
        nfa: &Nfa,
        literals: &[LiteralPattern],
        bridge: Option<&FuzzyBridge>,
    ) -> bool {
        // Fast path: single exact literal lookbehind (no fuzzy matching)
        // This is very common and can be done with simple string comparison
        if literals.len() == 1 {
            let lit = &literals[0];
            let max_edits = lit
                .limits
                .as_ref()
                .map_or(0, |l| l.get_edits().unwrap_or(0));

            if max_edits == 0 {
                // Exact match - just compare bytes
                let pattern = &lit.text;
                let pattern_bytes = pattern.as_bytes();
                if pos >= pattern_bytes.len() {
                    let start = pos - pattern_bytes.len();
                    if &text.as_bytes()[start..pos] == pattern_bytes {
                        return true;
                    }
                }
                return false;
            }
        }

        // Calculate the length range using pre-built bridge
        let (min_char_len, max_char_len) = nfa.length_range(|pattern_idx| {
            bridge
                .and_then(|b| {
                    let char_len = b.pattern_char_len(pattern_idx)?;
                    let max_edits = b.pattern_max_edits(pattern_idx).unwrap_or(0);
                    Some((char_len, max_edits))
                })
                .or_else(|| {
                    // For Char states (not in bridge), use pattern length with 0 edits
                    literals
                        .get(pattern_idx)
                        .map(|l| (l.text.chars().count(), 0))
                })
        });

        let sub_matcher = Matcher::new(
            nfa,
            bridge,
            0,
            MatcherConfig {
                unanchored: false,
                ..self.config.clone()
            },
            self.handlers,
        );

        // For fixed-length patterns, only try exact position
        // This is O(1) - we just need to find the byte offset for one position
        if let Some(max) = max_char_len
            && min_char_len == max
        {
            // Fixed length - compute the exact starting byte position
            // by counting back min_char_len characters from pos
            let text_before = &text[..pos];
            let start_byte = Self::nth_char_from_end_byte_offset(text_before, min_char_len);
            if let Some(start) = start_byte
                && let Some(m) = sub_matcher.find_at(text, start)
                && m.end == pos
            {
                return true;
            }
            return false;
        }

        // Variable length - need to compute char offsets for the search range
        let text_before = &text[..pos];
        let num_chars = text_before.chars().count();

        // Quick reject
        if min_char_len > num_chars {
            return false;
        }

        let max_char_search = max_char_len.unwrap_or(num_chars.min(256));

        // Only compute char_offsets for the positions we need to check
        let search_start_char = num_chars.saturating_sub(max_char_search);
        let search_end_char = num_chars.saturating_sub(min_char_len);

        // Collect only the byte offsets we need
        let mut char_count = 0;
        let mut positions = Vec::new();
        for (byte_idx, _) in text_before.char_indices() {
            if char_count >= search_start_char && char_count <= search_end_char {
                positions.push(byte_idx);
            }
            char_count += 1;
            if char_count > search_end_char {
                break;
            }
        }

        // Try positions from longest match first
        for start_byte in positions.into_iter().rev() {
            if let Some(m) = sub_matcher.find_at(text, start_byte)
                && m.end == pos
            {
                return true;
            }
        }

        false
    }

    /// Get the byte offset of the nth character from the end of a string.
    /// Returns None if there aren't enough characters.
    /// This is O(n) where n is the lookbehind length, not the text length.
    fn nth_char_from_end_byte_offset(s: &str, n: usize) -> Option<usize> {
        if n == 0 {
            return Some(s.len());
        }
        // Walk backwards from the end, counting characters
        // This is O(n) where n is the number of chars to skip
        let bytes = s.as_bytes();
        let mut byte_pos = bytes.len();
        let mut chars_seen = 0;

        while byte_pos > 0 && chars_seen < n {
            byte_pos -= 1;
            // Check if this is a UTF-8 start byte (not a continuation byte 10xxxxxx)
            if bytes[byte_pos] & 0b1100_0000 != 0b1000_0000 {
                chars_seen += 1;
            }
        }

        if chars_seen == n {
            Some(byte_pos)
        } else {
            None
        }
    }
}

/// Compute the Levenshtein edit distance between two strings.
/// Optimized with stack allocation for small strings (common in backreferences).
#[cfg(test)]
#[inline]
fn edit_distance(s1: &str, s2: &str) -> usize {
    edit_distance_bounded(s1, s2, NumEdits::MAX).into()
}

/// Bounded edit distance with early termination when distance exceeds `max_edits`.
/// Returns `usize::MAX` if distance would exceed `max_edits`.
#[inline]
fn edit_distance_bounded(s1: &str, s2: &str, max_edits: NumEdits) -> Distance {
    // Quick check: length difference gives lower bound on edit distance
    let len_diff = s1.len().abs_diff(s2.len());
    if len_diff > max_edits as usize {
        return Distance::MAX;
    }

    // Fast path for ASCII strings (common case)
    if s1.is_ascii() && s2.is_ascii() {
        return edit_distance_ascii_bounded(s1.as_bytes(), s2.as_bytes(), max_edits);
    }

    // Unicode path
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();

    // Recheck with char length
    let char_diff = s1_chars.len().abs_diff(s2_chars.len());
    if char_diff > max_edits as usize {
        return Distance::MAX;
    }

    edit_distance_slice_bounded(&s1_chars, &s2_chars, max_edits as usize)
}

/// Bounded edit distance for ASCII byte slices with early termination.
#[inline]
fn edit_distance_ascii_bounded(s1: &[u8], s2: &[u8], max_edits: NumEdits) -> Distance {
    let m = s1.len();
    let n = s2.len();

    if m == 0 {
        return if n <= max_edits.into() {
            n as Distance
        } else {
            Distance::MAX
        };
    }
    if n == 0 {
        return if m <= max_edits.into() {
            m as Distance
        } else {
            Distance::MAX
        };
    }

    // Stack allocation for small strings (covers most backreference cases)
    const STACK_LIMIT: usize = 64;
    if n < STACK_LIMIT {
        let mut prev = [0usize; STACK_LIMIT];
        let mut curr = [0usize; STACK_LIMIT];

        for j in 0..=n {
            prev[j] = j;
        }

        for i in 1..=m {
            curr[0] = i;
            let mut row_min = curr[0];
            for j in 1..=n {
                let cost = usize::from(s1[i - 1] != s2[j - 1]);
                curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
                row_min = row_min.min(curr[j]);
            }
            // Early termination: if minimum in row exceeds max_edits, distance will too
            if row_min > max_edits.into() {
                return Distance::MAX;
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        return prev[n] as Distance;
    }

    // Heap allocation for larger strings
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=n {
            let cost = usize::from(s1[i - 1] != s2[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(curr[j]);
        }
        if row_min > max_edits.into() {
            return Distance::MAX;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n] as Distance
}

/// Bounded edit distance for char slices with early termination.
#[inline]
fn edit_distance_slice_bounded(s1: &[char], s2: &[char], max_edits: usize) -> Distance {
    let m = s1.len();
    let n = s2.len();

    if m == 0 {
        return if n <= max_edits {
            n as Distance
        } else {
            Distance::MAX
        };
    }
    if n == 0 {
        return if m <= max_edits {
            m as Distance
        } else {
            Distance::MAX
        };
    }

    // Stack allocation for small strings
    const STACK_LIMIT: usize = 64;
    if n < STACK_LIMIT {
        let mut prev = [0usize; STACK_LIMIT];
        let mut curr = [0usize; STACK_LIMIT];

        for j in 0..=n {
            prev[j] = j;
        }

        for i in 1..=m {
            curr[0] = i;
            let mut row_min = curr[0];
            for j in 1..=n {
                let cost = usize::from(s1[i - 1] != s2[j - 1]);
                curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
                row_min = row_min.min(curr[j]);
            }
            if row_min > max_edits {
                return Distance::MAX;
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        return prev[n] as Distance;
    }

    // Heap allocation for larger strings
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=n {
            let cost = usize::from(s1[i - 1] != s2[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(curr[j]);
        }
        if row_min > max_edits {
            return Distance::MAX;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n] as Distance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::build_nfa;
    use crate::ir::lower;
    use crate::parser::parse;

    fn make_matcher(pattern: &str) -> (Nfa, Option<FuzzyBridge>, usize, HandlerMap) {
        let ast = parse(pattern).unwrap();
        let hir = lower(&ast, 0);
        let (nfa, literals) = build_nfa(&hir);

        let bridge = FuzzyBridge::new(&literals, None, None, false);

        // Count capture groups from AST
        let capture_count = count_captures(&ast);

        (nfa, bridge, capture_count, HandlerMap::default())
    }

    fn count_captures(ast: &crate::parser::Ast) -> usize {
        use crate::parser::Ast;
        match ast {
            Ast::Group { index, expr, .. } => (*index).max(count_captures(expr)),
            Ast::NonCapturingGroup { expr, .. } => count_captures(expr),
            Ast::Concat(parts) => parts.iter().map(count_captures).max().unwrap_or(0),
            Ast::Alternation(alts) => alts.iter().map(count_captures).max().unwrap_or(0),
            Ast::Quantified { expr, .. } => count_captures(expr),
            Ast::Lookahead { expr, .. } => count_captures(expr),
            Ast::Lookbehind { expr, .. } => count_captures(expr),
            _ => 0,
        }
    }

    #[test]
    fn test_simple_match() {
        let (nfa, bridge, captures, handlers) = make_matcher("hello");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        let result = matcher.find("hello world");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn test_find_no_cache() {
        let (nfa, bridge, captures, handlers) = make_matcher("hello");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        let result = matcher.find_no_cache("hello world");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn test_find_no_cache_fuzzy() {
        let (nfa, bridge, captures, handlers) = make_matcher("hello~1");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        let result = matcher.find_no_cache("hallo world");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn test_char_class() {
        let (nfa, bridge, captures, handlers) = make_matcher("[a-z]+");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        let result = matcher.find("123abc456");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(&"123abc456"[m.start..m.end], "abc");
    }

    #[test]
    fn test_capture_group() {
        let (nfa, bridge, captures, handlers) = make_matcher("(abc)");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        let result = matcher.find("xyzabc123");
        assert!(result.is_some());
        let m = result.unwrap();
        assert!(m.captures.get(1).is_some());
    }

    #[test]
    fn test_anchors() {
        let (nfa, bridge, captures, handlers) = make_matcher("^hello");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        assert!(matcher.find("hello world").is_some());
        assert!(matcher.find("say hello").is_none());
    }

    #[test]
    fn test_alternation() {
        let (nfa, bridge, captures, handlers) = make_matcher("cat|dog");
        let matcher = Matcher::new(
            &nfa,
            bridge.as_ref(),
            captures,
            MatcherConfig::default(),
            &handlers,
        );

        assert!(matcher.find("I have a cat").is_some());
        assert!(matcher.find("I have a dog").is_some());
        assert!(matcher.find("I have a bird").is_none());
    }

    #[test]
    fn test_edit_distance() {
        // Exact matches
        assert_eq!(edit_distance("hello", "hello"), 0);
        assert_eq!(edit_distance("", ""), 0);

        // Single edits
        assert_eq!(edit_distance("hello", "hallo"), 1); // substitution
        assert_eq!(edit_distance("hello", "hell"), 1); // deletion
        assert_eq!(edit_distance("hello", "helloo"), 1); // insertion
        assert_eq!(edit_distance("cat", "hat"), 1); // substitution

        // Multiple edits
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("saturday", "sunday"), 3);

        // Edge cases
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("a", "b"), 1);

        // Verify Bitap matches DP for various lengths
        for (s1, s2, expected) in [
            ("fox", "box", 1),
            ("quick", "quack", 1),
            ("brown", "brawn", 1),
            ("test", "taste", 2),
            ("abc", "xyz", 3),
        ] {
            assert_eq!(edit_distance(s1, s2), expected, "{s1} vs {s2}");
        }
    }
}
