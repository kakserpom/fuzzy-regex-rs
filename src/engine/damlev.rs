//! `DamLev` automaton for efficient fuzzy string matching.
//!
//! This implements a `DamLev` NFA that can find all approximate matches
//! of a pattern in a text with bounded edit distance in O(N × m × k) time.

#![allow(
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::too_many_lines
)]

/// Fast case-insensitive character conversion.
/// Uses ASCII fast-path to avoid ToLowercase iterator allocation for common case.
#[inline]
fn to_lower_char(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

use super::hash::{FxHashMap, FxHashSet};

/// Reusable buffers for NFA search to avoid allocations.
/// Create once and pass to search methods for reuse.
#[derive(Default, Debug)]
pub struct SearchBuffers {
    /// Active states during search
    active: Vec<ActiveState>,
    /// Next iteration's active states
    next_active: Vec<ActiveState>,
    /// Seen states for epsilon closure
    seen_set: FxHashSet<(State, usize)>,
    /// Deduplication map
    deduped: FxHashMap<(usize, usize, bool), ActiveState>,
    /// Match results
    matches: FxHashMap<(usize, usize), DamLevMatch>,
}

impl SearchBuffers {
    /// Create new buffers with default capacity.
    #[must_use]
    pub fn new() -> Self {
        SearchBuffers {
            active: Vec::with_capacity(32),
            next_active: Vec::with_capacity(32),
            seen_set: FxHashSet::default(),
            deduped: FxHashMap::default(),
            matches: FxHashMap::default(),
        }
    }

    /// Clear all buffers for reuse.
    pub fn clear(&mut self) {
        self.active.clear();
        self.next_active.clear();
        self.seen_set.clear();
        self.deduped.clear();
        self.matches.clear();
    }
}

/// Edit operation limits.
#[derive(Debug, Clone, Default)]
pub struct EditLimits {
    /// Maximum total number of edit operations allowed.
    pub max_edits: u8,
    /// Maximum number of insertion edits allowed (None = unlimited up to `max_edits`).
    pub max_insertions: Option<u8>,
    /// Maximum number of deletion edits allowed (None = unlimited up to `max_edits`).
    pub max_deletions: Option<u8>,
    /// Maximum number of substitution edits allowed (None = unlimited up to `max_edits`).
    pub max_substitutions: Option<u8>,
    /// Maximum number of transposition edits allowed (None = unlimited up to `max_edits`).
    pub max_swaps: Option<u8>,
}

impl EditLimits {
    /// Create edit limits with only a maximum total edit count.
    #[must_use]
    pub fn new(max_edits: u8) -> Self {
        EditLimits {
            max_edits,
            max_insertions: None,
            max_deletions: None,
            max_substitutions: None,
            max_swaps: None,
        }
    }

    /// Create edit limits with specific limits for each operation type.
    #[must_use]
    pub fn with_limits(
        max_edits: u8,
        max_insertions: Option<u8>,
        max_deletions: Option<u8>,
        max_substitutions: Option<u8>,
        max_swaps: Option<u8>,
    ) -> Self {
        EditLimits {
            max_edits,
            max_insertions,
            max_deletions,
            max_substitutions,
            max_swaps,
        }
    }
}

/// A match found by the `DamLev` automaton.
#[derive(Debug, Clone, Copy)]
pub struct DamLevMatch {
    /// Start position of the match (byte offset, inclusive).
    pub start: usize,
    /// End position of the match (byte offset, exclusive).
    pub end: usize,
    /// Number of insertion edits in this match.
    pub insertions: u8,
    /// Number of deletion edits in this match.
    pub deletions: u8,
    /// Number of substitution edits in this match.
    pub substitutions: u8,
    /// Number of transposition edits in this match.
    pub swaps: u8,
    /// Similarity score (0.0 to 1.0).
    pub similarity: f32,
}

impl DamLevMatch {
    /// Returns the total number of edit operations in this match.
    #[must_use]
    pub fn total_edits(&self) -> u8 {
        self.insertions
            .saturating_add(self.deletions)
            .saturating_add(self.substitutions)
            .saturating_add(self.swaps)
    }
}

/// State in the `DamLev` NFA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct State {
    /// Position in pattern (`0..=pattern_len`).
    pos: usize,
    /// Cached total edit count (sum of insertions + deletions + substitutions + swaps).
    total_edits: u8,
    /// Number of insertions used.
    insertions: u8,
    /// Number of deletions used.
    deletions: u8,
    /// Number of substitutions used.
    substitutions: u8,
    /// Number of swaps (transpositions) used.
    swaps: u8,
    /// If true, skip processing on next character (used for transposition which consumes 2 chars).
    skip_next: bool,
}

impl State {
    fn new(pos: usize) -> Self {
        State {
            pos,
            total_edits: 0,
            insertions: 0,
            deletions: 0,
            substitutions: 0,
            swaps: 0,
            skip_next: false,
        }
    }

    fn advance_match(&self) -> Self {
        State {
            pos: self.pos + 1,
            skip_next: false,
            total_edits: self.total_edits,
            ..*self
        }
    }

    fn advance_substitution(&self) -> Self {
        State {
            pos: self.pos + 1,
            substitutions: self.substitutions + 1,
            total_edits: self.total_edits + 1,
            skip_next: false,
            ..*self
        }
    }

    fn advance_deletion(&self) -> Self {
        State {
            pos: self.pos + 1,
            deletions: self.deletions + 1,
            total_edits: self.total_edits + 1,
            skip_next: false,
            ..*self
        }
    }

    fn advance_insertion(&self) -> Self {
        State {
            insertions: self.insertions + 1,
            total_edits: self.total_edits + 1,
            skip_next: false,
            ..*self
        }
    }

    /// Advance by transposition (swap two adjacent characters).
    /// Consumes 2 pattern chars and 2 text chars for 1 edit.
    fn advance_swap(&self) -> Self {
        State {
            pos: self.pos + 2,
            swaps: self.swaps + 1,
            total_edits: self.total_edits + 1,
            skip_next: true, // Skip next text char since transposition consumes 2
            ..*self
        }
    }
}

/// Active state tracking match start position.
#[derive(Debug, Clone, Copy)]
struct ActiveState {
    state: State,
    start_byte: usize,
    start_char: usize,
}

/// `DamLev` NFA for fuzzy pattern matching.
#[derive(Debug)]
pub struct DamLevNfa {
    pattern: String,
    pattern_chars: Vec<char>,
    limits: EditLimits,
    case_insensitive: bool,
    /// Beam width for state pruning - limits state explosion.
    /// Default: `pattern_len` * 4 (adapts to pattern size).
    beam_width: usize,
}

impl DamLevNfa {
    /// Create a new `DamLev` NFA for the given pattern.
    #[must_use]
    pub fn new(pattern: &str, limits: EditLimits, case_insensitive: bool) -> Self {
        let pattern_chars: Vec<char> = if case_insensitive {
            pattern.to_lowercase().chars().collect()
        } else {
            pattern.chars().collect()
        };

        // Beam width scales with pattern length and max_edits
        // Larger patterns need more states, but we cap it to prevent explosion
        let beam_width =
            ((pattern_chars.len() + 1) * (limits.max_edits as usize + 1) * 2).clamp(32, 256);

        DamLevNfa {
            pattern: pattern.to_string(),
            pattern_chars,
            limits,
            case_insensitive,
            beam_width,
        }
    }

    /// Returns the original pattern string.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Check if we can do an insertion.
    fn can_insert(&self, state: &State) -> bool {
        if state.total_edits >= self.limits.max_edits {
            return false;
        }
        self.limits
            .max_insertions
            .is_none_or(|max| state.insertions < max)
    }

    /// Check if we can do a deletion.
    fn can_delete(&self, state: &State) -> bool {
        if state.total_edits >= self.limits.max_edits {
            return false;
        }
        self.limits
            .max_deletions
            .is_none_or(|max| state.deletions < max)
    }

    /// Check if we can do a substitution.
    fn can_substitute(&self, state: &State) -> bool {
        if state.total_edits >= self.limits.max_edits {
            return false;
        }
        self.limits
            .max_substitutions
            .is_none_or(|max| state.substitutions < max)
    }

    /// Check if we can do a swap (transposition).
    fn can_swap(&self, state: &State) -> bool {
        if state.total_edits >= self.limits.max_edits {
            return false;
        }
        self.limits.max_swaps.is_none_or(|max| state.swaps < max)
    }

    /// Add epsilon closure (deletion transitions) to a set of states.
    /// Takes a reusable `HashSet` to avoid allocation on every call.
    /// The `HashSet` is used internally for deduplication and will be modified.
    fn epsilon_closure(&self, states: &mut Vec<ActiveState>, seen: &mut FxHashSet<(State, usize)>) {
        self.epsilon_closure_from(states, 0, seen);
    }

    /// Add epsilon closure starting from a specific index in the states vector.
    /// The `HashSet` should be pre-populated with existing states for deduplication.
    fn epsilon_closure_from(
        &self,
        states: &mut Vec<ActiveState>,
        start_idx: usize,
        seen: &mut FxHashSet<(State, usize)>,
    ) {
        let mut i = start_idx;
        while i < states.len() {
            let active = &states[i];
            let state = active.state;

            // Try deletion: skip pattern character without consuming text
            if state.pos < self.pattern_chars.len() && self.can_delete(&state) {
                let new_state = state.advance_deletion();
                let key = (new_state, active.start_byte);

                if !seen.contains(&key) {
                    seen.insert(key);
                    states.push(ActiveState {
                        state: new_state,
                        start_byte: active.start_byte,
                        start_char: active.start_char,
                    });
                }
            }

            i += 1;
        }
    }

    /// Calculate similarity score using normalized edit distance.
    fn calc_similarity(&self, state: &State) -> f32 {
        let pattern_len = self.pattern_chars.len() as f32;
        if pattern_len == 0.0 {
            return 1.0;
        }

        let edit_distance = f32::from(state.total_edits);
        // Matched text length = pattern_len + insertions - deletions
        let matched_len = pattern_len + f32::from(state.insertions) - f32::from(state.deletions);
        let max_len = pattern_len.max(matched_len).max(1.0);

        // Normalized DamLev similarity
        (1.0 - edit_distance / max_len).max(0.0)
    }

    /// Calculate the maximum possible similarity a state can achieve.
    /// Used for early pruning - if max possible < threshold, prune the state.
    ///
    /// This is conservative: we only prune if the state CANNOT reach the threshold.
    /// The actual similarity is: (1.0 - edits / max(`pattern_len`, `matched_len`))
    /// where `matched_len` = `pattern_len` + insertions - deletions.
    ///
    /// For early pruning, we use the most optimistic denominator (largest possible),
    /// which gives the highest possible similarity.
    #[inline]
    fn max_possible_similarity(&self, state: &State) -> f32 {
        let pattern_len = self.pattern_chars.len() as f32;
        if pattern_len == 0.0 {
            return 1.0;
        }

        let min_edits = f32::from(state.total_edits);

        // Current matched length estimate (can grow with more insertions)
        let current_matched_len =
            pattern_len + f32::from(state.insertions) - f32::from(state.deletions);

        // The max possible denominator is max(pattern_len, current_matched_len + remaining_edits)
        // We're conservative: use the largest plausible denominator
        let remaining_edits = f32::from(self.limits.max_edits - state.total_edits);
        let max_denominator = pattern_len.max(current_matched_len + remaining_edits);

        // Best case: no more edits needed, largest denominator
        (1.0 - min_edits / max_denominator).max(0.0)
    }

    /// Apply beam pruning to active states - keep only the best states.
    /// Sorts by `total_edits` (lower is better) and truncates to `beam_width`.
    #[inline]
    fn beam_prune(&self, states: &mut Vec<ActiveState>) {
        if states.len() > self.beam_width * 2 {
            // Use select_nth_unstable to partition: states with <= beam_width edits
            // go to the front, the rest go to the back. This is O(n) instead of
            // O(n log n) full sort.
            let (kept, _, _) =
                states.select_nth_unstable_by_key(self.beam_width, |s| s.state.total_edits);
            // Sort only the kept portion (beam_width elements) for stable ordering
            kept.sort_by_key(|s| s.state.total_edits);
            states.truncate(self.beam_width);
        }
    }

    /// Find the first match in the text.
    ///
    /// Returns the earliest match found, or None if no match exists.
    #[must_use]
    pub fn find_first(&self, text: &str, threshold: f32) -> Option<DamLevMatch> {
        let mut buffers = SearchBuffers::new();
        self.find_first_buffered(text, threshold, &mut buffers)
    }

    /// Find the first match using pre-allocated buffers.
    ///
    /// Returns the earliest match found, or None if no match exists.
    /// Picks the leftmost match, and among matches starting at the same
    /// position prefer the best one: fewest edits, then shortest span. This
    /// mirrors mrab-regex's default `search` (which reports the minimal-error
    /// match at the leftmost position) rather than whichever alignment the
    /// NFA happens to accept first (e.g. a 2-edit "hetl" over a 1-edit
    /// "hetlo" for `(?:hello){i<=1,d<=1,s<=1}`).
    #[must_use]
    pub fn find_first_buffered(
        &self,
        text: &str,
        threshold: f32,
        buffers: &mut SearchBuffers,
    ) -> Option<DamLevMatch> {
        self.find_all_buffered(text, threshold, buffers)
            .into_iter()
            .min_by(|a, b| {
                a.start
                    .cmp(&b.start)
                    .then_with(|| a.total_edits().cmp(&b.total_edits()))
                    .then_with(|| a.end.cmp(&b.end))
            })
    }

    /// Find all matches in the text.
    ///
    /// Returns matches organized by (`start_byte`, `end_byte`) with the best match for each span.
    #[must_use]
    pub fn find_all(&self, text: &str, threshold: f32) -> Vec<DamLevMatch> {
        if self.pattern_chars.is_empty() {
            // Empty pattern matches everywhere
            return vec![DamLevMatch {
                start: 0,
                end: 0,
                insertions: 0,
                deletions: 0,
                substitutions: 0,
                swaps: 0,
                similarity: 1.0,
            }];
        }

        let text_chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut matches: FxHashMap<(usize, usize), DamLevMatch> = FxHashMap::default();

        // Reusable FxHashSet to avoid allocation per character
        let mut seen_set: FxHashSet<(State, usize)> = FxHashSet::default();

        // Reusable FxHashMap for deduplication
        let mut deduped: FxHashMap<(usize, usize, bool), ActiveState> = FxHashMap::default();

        // Active states: each contains (State, start_byte_pos, start_char_pos)
        let mut active: Vec<ActiveState> = Vec::new();

        // Process text character by character
        for (char_idx, &(byte_pos, text_char)) in text_chars.iter().enumerate() {
            let text_char = if self.case_insensitive {
                to_lower_char(text_char)
            } else {
                text_char
            };

            // Get next text char for transposition detection
            let next_text_char = text_chars.get(char_idx + 1).map(|&(_, c)| {
                if self.case_insensitive {
                    to_lower_char(c)
                } else {
                    c
                }
            });

            // Start a new potential match at this position
            let initial = ActiveState {
                state: State::new(0),
                start_byte: byte_pos,
                start_char: char_idx,
            };
            active.push(initial);

            // Add epsilon closure for new state - run directly on active
            {
                // Track where new states start
                let start_idx = active.len() - 1;
                // Populate seen_set with existing states
                seen_set.clear();
                for a in &active {
                    seen_set.insert((a.state, a.start_byte));
                }
                // Run epsilon closure starting from the new state
                self.epsilon_closure_from(&mut active, start_idx, &mut seen_set);
            }

            // Compute next states
            let mut next_active: Vec<ActiveState> = Vec::new();

            for active_state in &active {
                let state = active_state.state;

                // If this state is marked skip_next (from a transposition), just clear the flag
                if state.skip_next {
                    let mut continued = state;
                    continued.skip_next = false;
                    next_active.push(ActiveState {
                        state: continued,
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                    continue;
                }

                if state.pos < self.pattern_chars.len() {
                    let pattern_char = self.pattern_chars[state.pos];

                    // Match transition
                    if text_char == pattern_char {
                        let new_state = state.advance_match();
                        next_active.push(ActiveState {
                            state: new_state,
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    // Substitution transition
                    if text_char != pattern_char && self.can_substitute(&state) {
                        let new_state = state.advance_substitution();
                        next_active.push(ActiveState {
                            state: new_state,
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    // Transposition transition: pattern[pos:pos+2] matches text[idx:idx+2] in reverse
                    // e.g., pattern "ab" matches text "ba"
                    if let Some(next_char) = next_text_char
                        && state.pos + 1 < self.pattern_chars.len()
                        && self.can_swap(&state)
                    {
                        let next_pattern_char = self.pattern_chars[state.pos + 1];
                        // Check if pattern[pos]=next_text and pattern[pos+1]=current_text (swapped)
                        if pattern_char == next_char
                            && next_pattern_char == text_char
                            && pattern_char != next_pattern_char
                        {
                            let new_state = state.advance_swap();
                            next_active.push(ActiveState {
                                state: new_state,
                                start_byte: active_state.start_byte,
                                start_char: active_state.start_char,
                            });
                        }
                    }
                }

                // Insertion transition (consume text char, stay at same pattern pos)
                if self.can_insert(&state) {
                    let new_state = state.advance_insertion();
                    next_active.push(ActiveState {
                        state: new_state,
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                }
            }

            // Add epsilon closure (deletions)
            seen_set.clear();
            self.epsilon_closure(&mut next_active, &mut seen_set);

            // Deduplicate: keep best state for each (pos, start_byte, skip_next)
            deduped.clear();
            for active_state in next_active {
                // Early pruning: skip states that can't reach threshold
                if self.max_possible_similarity(&active_state.state) < threshold {
                    continue;
                }

                let key = (
                    active_state.state.pos,
                    active_state.start_byte,
                    active_state.state.skip_next,
                );
                deduped
                    .entry(key)
                    .and_modify(|existing| {
                        // Keep state with fewer edits
                        if active_state.state.total_edits < existing.state.total_edits {
                            *existing = active_state.clone();
                        }
                    })
                    .or_insert(active_state);
            }

            active.clear();
            active.extend(deduped.values().cloned());

            // Beam pruning: if too many states, keep only the best ones
            self.beam_prune(&mut active);

            // Check for accepting states (reached end of pattern)
            let end_byte = text_chars.get(char_idx + 1).map_or(text.len(), |(b, _)| *b);

            for active_state in &active {
                if active_state.state.pos == self.pattern_chars.len()
                    && !active_state.state.skip_next
                {
                    let sim = self.calc_similarity(&active_state.state);
                    if sim >= threshold {
                        let key = (active_state.start_byte, end_byte);
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: end_byte,
                            insertions: active_state.state.insertions,
                            deletions: active_state.state.deletions,
                            substitutions: active_state.state.substitutions,
                            swaps: active_state.state.swaps,
                            similarity: sim,
                        };

                        matches
                            .entry(key)
                            .and_modify(|existing| {
                                if m.similarity > existing.similarity {
                                    *existing = m.clone();
                                }
                            })
                            .or_insert(m);
                    }
                }
            }

            // Remove states that reached the end of pattern (they've been recorded)
            // Also prune states that have fallen too far behind (can't possibly match)
            let max_window = self.pattern_chars.len() + self.limits.max_edits as usize;
            active.retain(|a| {
                (a.state.pos < self.pattern_chars.len() || a.state.skip_next)
                    && (char_idx + 1 - a.start_char) <= max_window
            });
        }

        // Handle remaining states that might match with deletions at the end
        for active_state in &active {
            let state = active_state.state;
            if state.skip_next {
                continue; // Can't complete if waiting for next char
            }
            let remaining = (self.pattern_chars.len() - state.pos) as u8;

            // Can we delete all remaining pattern chars?
            if remaining <= self.limits.max_edits - state.total_edits {
                let dels_needed = remaining;
                let total_dels = state.deletions + dels_needed;

                if self
                    .limits
                    .max_deletions
                    .is_none_or(|max| total_dels <= max)
                {
                    let final_state = State {
                        pos: self.pattern_chars.len(),
                        deletions: total_dels,
                        total_edits: state.total_edits + dels_needed,
                        ..state
                    };

                    let sim = self.calc_similarity(&final_state);
                    if sim >= threshold {
                        let key = (active_state.start_byte, text.len());
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: text.len(),
                            insertions: final_state.insertions,
                            deletions: final_state.deletions,
                            substitutions: final_state.substitutions,
                            swaps: final_state.swaps,
                            similarity: sim,
                        };

                        matches
                            .entry(key)
                            .and_modify(|existing| {
                                if m.similarity > existing.similarity {
                                    *existing = m.clone();
                                }
                            })
                            .or_insert(m);
                    }
                }
            }
        }

        matches.into_values().collect()
    }

    /// Find all matches using pre-allocated buffers (streaming, no text_chars Vec).
    ///
    /// Like `find_all` but avoids the per-call allocation of `text_chars` and uses
    /// the caller-supplied `SearchBuffers` for all internal state. This is the
    /// preferred method when calling `find_all` repeatedly (e.g. in the fuzzy
    /// bridge's `search_all`).
    #[must_use]
    pub fn find_all_buffered(
        &self,
        text: &str,
        threshold: f32,
        buffers: &mut SearchBuffers,
    ) -> Vec<DamLevMatch> {
        // Clear buffers for reuse
        buffers.clear();

        if self.pattern_chars.is_empty() {
            return vec![DamLevMatch {
                start: 0,
                end: 0,
                insertions: 0,
                deletions: 0,
                substitutions: 0,
                swaps: 0,
                similarity: 1.0,
            }];
        }

        // Stream through chars without peekable overhead
        let mut char_iter = text.char_indices();
        let mut next_char = char_iter.next();

        let mut char_idx = 0usize;
        while let Some((byte_pos, raw_char)) = next_char {
            next_char = char_iter.next();
            let text_char = if self.case_insensitive {
                to_lower_char(raw_char)
            } else {
                raw_char
            };

            // Next char for transposition detection and end_byte calculation
            let next_info = next_char.map(|(next_byte, next_char)| {
                let c = if self.case_insensitive {
                    to_lower_char(next_char)
                } else {
                    next_char
                };
                (next_byte, c)
            });
            let next_text_char = next_info.map(|(_, c)| c);
            let end_byte = next_info.map_or(text.len(), |(b, _)| b);

            // Start a new potential match at every position
            let initial = ActiveState {
                state: State::new(0),
                start_byte: byte_pos,
                start_char: char_idx,
            };
            buffers.active.push(initial);

            // Add epsilon closure for new state - run directly on active
            {
                let start_idx = buffers.active.len() - 1;
                buffers.seen_set.clear();
                for a in &buffers.active {
                    buffers.seen_set.insert((a.state, a.start_byte));
                }
                self.epsilon_closure_from(&mut buffers.active, start_idx, &mut buffers.seen_set);
            }

            // Process active states - reuse next_active buffer
            buffers.next_active.clear();

            for active_state in &buffers.active {
                let state = active_state.state;

                // If this state is marked skip_next (from a transposition), just clear the flag
                if state.skip_next {
                    let mut continued = state;
                    continued.skip_next = false;
                    buffers.next_active.push(ActiveState {
                        state: continued,
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                    continue;
                }

                if state.pos < self.pattern_chars.len() {
                    let pattern_char = self.pattern_chars[state.pos];

                    if text_char == pattern_char {
                        buffers.next_active.push(ActiveState {
                            state: state.advance_match(),
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    if text_char != pattern_char && self.can_substitute(&state) {
                        buffers.next_active.push(ActiveState {
                            state: state.advance_substitution(),
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    // Transposition transition
                    if let Some(next_char) = next_text_char
                        && state.pos + 1 < self.pattern_chars.len()
                        && self.can_swap(&state)
                    {
                        let next_pattern_char = self.pattern_chars[state.pos + 1];
                        if pattern_char == next_char
                            && next_pattern_char == text_char
                            && pattern_char != next_pattern_char
                        {
                            buffers.next_active.push(ActiveState {
                                state: state.advance_swap(),
                                start_byte: active_state.start_byte,
                                start_char: active_state.start_char,
                            });
                        }
                    }
                }

                if self.can_insert(&state) {
                    buffers.next_active.push(ActiveState {
                        state: state.advance_insertion(),
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                }
            }

            buffers.seen_set.clear();
            self.epsilon_closure(&mut buffers.next_active, &mut buffers.seen_set);

            // Deduplicate with early pruning
            buffers.deduped.clear();
            for active_state in buffers.next_active.drain(..) {
                // Early pruning: skip states that can't reach threshold
                if self.max_possible_similarity(&active_state.state) < threshold {
                    continue;
                }

                let key = (
                    active_state.state.pos,
                    active_state.start_byte,
                    active_state.state.skip_next,
                );
                buffers
                    .deduped
                    .entry(key)
                    .and_modify(|existing| {
                        if active_state.state.total_edits < existing.state.total_edits {
                            *existing = active_state.clone();
                        }
                    })
                    .or_insert(active_state);
            }

            buffers.active.clear();
            buffers.active.extend(buffers.deduped.values().cloned());

            // Beam pruning: limit state explosion
            self.beam_prune(&mut buffers.active);

            // Check for accepting states (end_byte already computed above from peek)
            for active_state in &buffers.active {
                if active_state.state.pos == self.pattern_chars.len()
                    && !active_state.state.skip_next
                {
                    let sim = self.calc_similarity(&active_state.state);
                    if sim >= threshold {
                        let key = (active_state.start_byte, end_byte);
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: end_byte,
                            insertions: active_state.state.insertions,
                            deletions: active_state.state.deletions,
                            substitutions: active_state.state.substitutions,
                            swaps: active_state.state.swaps,
                            similarity: sim,
                        };

                        buffers
                            .matches
                            .entry(key)
                            .and_modify(|existing| {
                                if m.similarity > existing.similarity {
                                    *existing = m;
                                }
                            })
                            .or_insert(m);
                    }
                }
            }

            // Prune states
            let max_window = self.pattern_chars.len() + self.limits.max_edits as usize;
            buffers.active.retain(|a| {
                (a.state.pos < self.pattern_chars.len() || a.state.skip_next)
                    && (char_idx + 1 - a.start_char) <= max_window
            });

            char_idx += 1;
        }

        // Handle remaining states at end of text
        for active_state in &buffers.active {
            let state = active_state.state;
            if state.skip_next {
                continue;
            }
            let remaining = self.pattern_chars.len() - state.pos;

            if remaining as u8 <= self.limits.max_edits - state.total_edits {
                let dels_needed = remaining as u8;
                let total_dels = state.deletions + dels_needed;

                if self
                    .limits
                    .max_deletions
                    .is_none_or(|max| total_dels <= max)
                {
                    let final_state = State {
                        pos: self.pattern_chars.len(),
                        deletions: total_dels,
                        total_edits: state.total_edits + dels_needed,
                        ..state
                    };

                    let sim = self.calc_similarity(&final_state);
                    if sim >= threshold {
                        let key = (active_state.start_byte, text.len());
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: text.len(),
                            insertions: final_state.insertions,
                            deletions: final_state.deletions,
                            substitutions: final_state.substitutions,
                            swaps: final_state.swaps,
                            similarity: sim,
                        };

                        buffers
                            .matches
                            .entry(key)
                            .and_modify(|existing| {
                                if m.similarity > existing.similarity {
                                    *existing = m;
                                }
                            })
                            .or_insert(m);
                    }
                }
            }
        }

        buffers.matches.drain().map(|(_, v)| v).collect()
    }

    /// Find up to `n` non-overlapping matches.
    ///
    /// Note: This is a simple implementation that finds all matches and takes the first `n`.
    /// For large texts with many matches, consider using the Bitap-based matcher instead.
    #[must_use]
    pub fn find_n(&self, text: &str, threshold: f32, n: usize) -> Vec<DamLevMatch> {
        if n == 0 {
            return Vec::new();
        }

        let mut all_matches = self.find_all(text, threshold);

        // Sort by start position
        all_matches.sort_by_key(|m| m.start);

        // Take non-overlapping matches up to limit
        let mut result = Vec::with_capacity(n.min(all_matches.len()));
        let mut last_end = 0;

        for m in all_matches {
            if m.start >= last_end {
                last_end = m.end;
                result.push(m);
                if result.len() >= n {
                    break;
                }
            }
        }

        result
    }

    /// Find all matches, but only start new potential matches at candidate positions.
    ///
    /// This is an optimization: instead of starting a new match at every character,
    /// we only start at positions identified by a prefilter.
    #[must_use]
    pub fn find_all_with_candidates(
        &self,
        text: &str,
        threshold: f32,
        candidates: &FxHashSet<usize>,
    ) -> Vec<DamLevMatch> {
        let mut buffers = SearchBuffers::new();
        self.find_all_with_candidates_buffered(text, threshold, candidates, &mut buffers)
    }

    /// Find all matches using pre-allocated buffers to avoid allocations.
    pub fn find_all_with_candidates_buffered(
        &self,
        text: &str,
        threshold: f32,
        candidates: &FxHashSet<usize>,
        buffers: &mut SearchBuffers,
    ) -> Vec<DamLevMatch> {
        // Clear buffers for reuse
        buffers.clear();

        if self.pattern_chars.is_empty() {
            return vec![DamLevMatch {
                start: 0,
                end: 0,
                insertions: 0,
                deletions: 0,
                substitutions: 0,
                swaps: 0,
                similarity: 1.0,
            }];
        }

        // Stream through chars without peekable overhead
        let mut char_iter = text.char_indices();
        let mut next_char = char_iter.next();

        let mut char_idx = 0usize;
        while let Some((byte_pos, raw_char)) = next_char {
            next_char = char_iter.next();
            let text_char = if self.case_insensitive {
                to_lower_char(raw_char)
            } else {
                raw_char
            };

            // Next char for transposition detection and end_byte calculation
            let next_info = next_char.map(|(next_byte, next_char)| {
                let c = if self.case_insensitive {
                    to_lower_char(next_char)
                } else {
                    next_char
                };
                (next_byte, c)
            });
            let next_text_char = next_info.map(|(_, c)| c);
            let end_byte = next_info.map_or(text.len(), |(b, _)| b);

            // Only start a new potential match if this is a candidate position
            if candidates.contains(&byte_pos) {
                let initial = ActiveState {
                    state: State::new(0),
                    start_byte: byte_pos,
                    start_char: char_idx,
                };
                buffers.active.push(initial);

                // Add epsilon closure for new state - run directly on active
                {
                    let start_idx = buffers.active.len() - 1;
                    buffers.seen_set.clear();
                    for a in &buffers.active {
                        buffers.seen_set.insert((a.state, a.start_byte));
                    }
                    self.epsilon_closure_from(
                        &mut buffers.active,
                        start_idx,
                        &mut buffers.seen_set,
                    );
                }
            }

            // Process active states - reuse next_active buffer
            buffers.next_active.clear();

            for active_state in &buffers.active {
                let state = active_state.state;

                // If this state is marked skip_next (from a transposition), just clear the flag
                if state.skip_next {
                    let mut continued = state;
                    continued.skip_next = false;
                    buffers.next_active.push(ActiveState {
                        state: continued,
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                    continue;
                }

                if state.pos < self.pattern_chars.len() {
                    let pattern_char = self.pattern_chars[state.pos];

                    if text_char == pattern_char {
                        buffers.next_active.push(ActiveState {
                            state: state.advance_match(),
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    if text_char != pattern_char && self.can_substitute(&state) {
                        buffers.next_active.push(ActiveState {
                            state: state.advance_substitution(),
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    // Transposition transition
                    if let Some(next_char) = next_text_char
                        && state.pos + 1 < self.pattern_chars.len()
                        && self.can_swap(&state)
                    {
                        let next_pattern_char = self.pattern_chars[state.pos + 1];
                        if pattern_char == next_char
                            && next_pattern_char == text_char
                            && pattern_char != next_pattern_char
                        {
                            buffers.next_active.push(ActiveState {
                                state: state.advance_swap(),
                                start_byte: active_state.start_byte,
                                start_char: active_state.start_char,
                            });
                        }
                    }
                }

                if self.can_insert(&state) {
                    buffers.next_active.push(ActiveState {
                        state: state.advance_insertion(),
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                }
            }

            buffers.seen_set.clear();
            self.epsilon_closure(&mut buffers.next_active, &mut buffers.seen_set);

            // Deduplicate with early pruning
            buffers.deduped.clear();
            for active_state in buffers.next_active.drain(..) {
                // Early pruning: skip states that can't reach threshold
                if self.max_possible_similarity(&active_state.state) < threshold {
                    continue;
                }

                let key = (
                    active_state.state.pos,
                    active_state.start_byte,
                    active_state.state.skip_next,
                );
                buffers
                    .deduped
                    .entry(key)
                    .and_modify(|existing| {
                        if active_state.state.total_edits < existing.state.total_edits {
                            *existing = active_state.clone();
                        }
                    })
                    .or_insert(active_state);
            }

            buffers.active.clear();
            buffers.active.extend(buffers.deduped.values().cloned());

            // Beam pruning: limit state explosion
            self.beam_prune(&mut buffers.active);

            // Check for accepting states (end_byte already computed above from peek)
            for active_state in &buffers.active {
                if active_state.state.pos == self.pattern_chars.len()
                    && !active_state.state.skip_next
                {
                    let sim = self.calc_similarity(&active_state.state);
                    if sim >= threshold {
                        let key = (active_state.start_byte, end_byte);
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: end_byte,
                            insertions: active_state.state.insertions,
                            deletions: active_state.state.deletions,
                            substitutions: active_state.state.substitutions,
                            swaps: active_state.state.swaps,
                            similarity: sim,
                        };

                        buffers
                            .matches
                            .entry(key)
                            .and_modify(|existing| {
                                if m.similarity > existing.similarity {
                                    *existing = m;
                                }
                            })
                            .or_insert(m);
                    }
                }
            }

            // Prune states
            let max_window = self.pattern_chars.len() + self.limits.max_edits as usize;
            buffers.active.retain(|a| {
                (a.state.pos < self.pattern_chars.len() || a.state.skip_next)
                    && (char_idx + 1 - a.start_char) <= max_window
            });

            char_idx += 1;
        }

        // Handle remaining states at end of text
        for active_state in &buffers.active {
            let state = active_state.state;
            if state.skip_next {
                continue;
            }
            let remaining = self.pattern_chars.len() - state.pos;

            if remaining as u8 <= self.limits.max_edits - state.total_edits {
                let dels_needed = remaining as u8;
                let total_dels = state.deletions + dels_needed;

                if self
                    .limits
                    .max_deletions
                    .is_none_or(|max| total_dels <= max)
                {
                    let final_state = State {
                        pos: self.pattern_chars.len(),
                        deletions: total_dels,
                        total_edits: state.total_edits + dels_needed,
                        ..state
                    };

                    let sim = self.calc_similarity(&final_state);
                    if sim >= threshold {
                        let key = (active_state.start_byte, text.len());
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: text.len(),
                            insertions: final_state.insertions,
                            deletions: final_state.deletions,
                            substitutions: final_state.substitutions,
                            swaps: final_state.swaps,
                            similarity: sim,
                        };

                        buffers
                            .matches
                            .entry(key)
                            .and_modify(|existing| {
                                if m.similarity > existing.similarity {
                                    *existing = m;
                                }
                            })
                            .or_insert(m);
                    }
                }
            }
        }

        buffers.matches.drain().map(|(_, v)| v).collect()
    }

    /// Find the first match using pre-allocated buffers, only starting at candidate positions.
    ///
    /// Returns the earliest match found, or None if no match exists.
    /// Picks the best match (highest similarity) at the leftmost position where a match is found.
    /// This is much faster than `find_all_with_candidates` when we only need the first match.
    pub fn find_first_with_candidates_buffered(
        &self,
        text: &str,
        threshold: f32,
        candidates: &FxHashSet<usize>,
        buffers: &mut SearchBuffers,
    ) -> Option<DamLevMatch> {
        // Clear buffers for reuse
        buffers.clear();

        if self.pattern_chars.is_empty() {
            return Some(DamLevMatch {
                start: 0,
                end: 0,
                insertions: 0,
                deletions: 0,
                substitutions: 0,
                swaps: 0,
                similarity: 1.0,
            });
        }

        // Stream through chars without peekable overhead
        let mut char_iter = text.char_indices();
        let mut next_char = char_iter.next();

        let mut char_idx = 0usize;
        while let Some((byte_pos, raw_char)) = next_char {
            next_char = char_iter.next();
            let text_char = if self.case_insensitive {
                to_lower_char(raw_char)
            } else {
                raw_char
            };

            // Next char for transposition detection and end_byte calculation
            let next_info = next_char.map(|(next_byte, next_char)| {
                let c = if self.case_insensitive {
                    to_lower_char(next_char)
                } else {
                    next_char
                };
                (next_byte, c)
            });
            let next_text_char = next_info.map(|(_, c)| c);
            let end_byte = next_info.map_or(text.len(), |(b, _)| b);

            // Only start a new potential match if this is a candidate position
            if candidates.contains(&byte_pos) {
                let initial = ActiveState {
                    state: State::new(0),
                    start_byte: byte_pos,
                    start_char: char_idx,
                };
                buffers.active.push(initial);

                // Add epsilon closure for new state - run directly on active
                {
                    let start_idx = buffers.active.len() - 1;
                    buffers.seen_set.clear();
                    for a in &buffers.active {
                        buffers.seen_set.insert((a.state, a.start_byte));
                    }
                    self.epsilon_closure_from(
                        &mut buffers.active,
                        start_idx,
                        &mut buffers.seen_set,
                    );
                }
            }

            // If no active states, continue to next char
            if buffers.active.is_empty() {
                char_idx += 1;
                continue;
            }

            // Process active states - reuse next_active buffer
            buffers.next_active.clear();

            for active_state in &buffers.active {
                let state = active_state.state;

                // If this state is marked skip_next (from a transposition), just clear the flag
                if state.skip_next {
                    let mut continued = state;
                    continued.skip_next = false;
                    buffers.next_active.push(ActiveState {
                        state: continued,
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                    continue;
                }

                if state.pos < self.pattern_chars.len() {
                    let pattern_char = self.pattern_chars[state.pos];

                    if text_char == pattern_char {
                        buffers.next_active.push(ActiveState {
                            state: state.advance_match(),
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    if text_char != pattern_char && self.can_substitute(&state) {
                        buffers.next_active.push(ActiveState {
                            state: state.advance_substitution(),
                            start_byte: active_state.start_byte,
                            start_char: active_state.start_char,
                        });
                    }

                    // Transposition transition
                    if let Some(next_char) = next_text_char
                        && state.pos + 1 < self.pattern_chars.len()
                        && self.can_swap(&state)
                    {
                        let next_pattern_char = self.pattern_chars[state.pos + 1];
                        if pattern_char == next_char
                            && next_pattern_char == text_char
                            && pattern_char != next_pattern_char
                        {
                            buffers.next_active.push(ActiveState {
                                state: state.advance_swap(),
                                start_byte: active_state.start_byte,
                                start_char: active_state.start_char,
                            });
                        }
                    }
                }

                if self.can_insert(&state) {
                    buffers.next_active.push(ActiveState {
                        state: state.advance_insertion(),
                        start_byte: active_state.start_byte,
                        start_char: active_state.start_char,
                    });
                }
            }

            buffers.seen_set.clear();
            self.epsilon_closure(&mut buffers.next_active, &mut buffers.seen_set);

            // Deduplicate with early pruning
            buffers.deduped.clear();
            for active_state in buffers.next_active.drain(..) {
                // Early pruning: skip states that can't reach threshold
                if self.max_possible_similarity(&active_state.state) < threshold {
                    continue;
                }

                let key = (
                    active_state.state.pos,
                    active_state.start_byte,
                    active_state.state.skip_next,
                );
                buffers
                    .deduped
                    .entry(key)
                    .and_modify(|existing| {
                        if active_state.state.total_edits < existing.state.total_edits {
                            *existing = active_state.clone();
                        }
                    })
                    .or_insert(active_state);
            }

            buffers.active.clear();
            buffers.active.extend(buffers.deduped.values().cloned());

            // Beam pruning: limit state explosion
            self.beam_prune(&mut buffers.active);

            // Check for accepting states - return best match at this position
            let mut best_match: Option<DamLevMatch> = None;
            for active_state in &buffers.active {
                if active_state.state.pos == self.pattern_chars.len()
                    && !active_state.state.skip_next
                {
                    let sim = self.calc_similarity(&active_state.state);
                    if sim >= threshold {
                        let m = DamLevMatch {
                            start: active_state.start_byte,
                            end: end_byte,
                            insertions: active_state.state.insertions,
                            deletions: active_state.state.deletions,
                            substitutions: active_state.state.substitutions,
                            swaps: active_state.state.swaps,
                            similarity: sim,
                        };
                        if best_match.as_ref().is_none_or(|best| sim > best.similarity) {
                            best_match = Some(m);
                        }
                    }
                }
            }
            if best_match.is_some() {
                return best_match;
            }

            // Prune states
            let max_window = self.pattern_chars.len() + self.limits.max_edits as usize;
            buffers.active.retain(|a| {
                (a.state.pos < self.pattern_chars.len() || a.state.skip_next)
                    && (char_idx + 1 - a.start_char) <= max_window
            });

            char_idx += 1;
        }

        // Check remaining states at end of text
        for active_state in &buffers.active {
            let state = active_state.state;
            if state.skip_next {
                continue;
            }
            let remaining = self.pattern_chars.len() - state.pos;

            if remaining as u8 <= self.limits.max_edits - state.total_edits {
                let dels_needed = remaining as u8;
                let total_dels = state.deletions + dels_needed;

                if self
                    .limits
                    .max_deletions
                    .is_none_or(|max| total_dels <= max)
                {
                    let final_state = State {
                        pos: self.pattern_chars.len(),
                        deletions: total_dels,
                        total_edits: state.total_edits + dels_needed,
                        ..state
                    };

                    let sim = self.calc_similarity(&final_state);
                    if sim >= threshold {
                        return Some(DamLevMatch {
                            start: active_state.start_byte,
                            end: text.len(),
                            insertions: final_state.insertions,
                            deletions: final_state.deletions,
                            substitutions: final_state.substitutions,
                            swaps: final_state.swaps,
                            similarity: sim,
                        });
                    }
                }
            }
        }

        None
    }

    /// Find the first match, stopping as soon as one is found.
    ///
    /// This is much faster than `find_all` when we only need the first match,
    /// especially when the match is found early in the text.
    #[must_use]
    pub fn find_first_with_candidates(
        &self,
        text: &str,
        threshold: f32,
        candidates: &FxHashSet<usize>,
    ) -> Option<DamLevMatch> {
        let mut buffers = SearchBuffers::new();
        self.find_first_with_candidates_buffered(text, threshold, candidates, &mut buffers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let nfa = DamLevNfa::new("hello", EditLimits::new(0), false);
        let matches = nfa.find_all("hello world", 0.8);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 5);
        assert_eq!(matches[0].total_edits(), 0);
    }

    #[test]
    fn test_one_substitution() {
        let nfa = DamLevNfa::new("hello", EditLimits::new(1), false);
        let matches = nfa.find_all("hallo world", 0.5);

        assert!(
            matches
                .iter()
                .any(|m| m.start == 0 && m.end == 5 && m.substitutions == 1)
        );
    }

    #[test]
    fn test_one_insertion() {
        let nfa = DamLevNfa::new("hello", EditLimits::new(1), false);
        let matches = nfa.find_all("heello world", 0.5);

        assert!(matches.iter().any(|m| m.start == 0 && m.insertions == 1));
    }

    #[test]
    fn test_one_deletion() {
        let nfa = DamLevNfa::new("hello", EditLimits::new(1), false);
        let matches = nfa.find_all("helo world", 0.5);

        assert!(matches.iter().any(|m| m.start == 0 && m.deletions == 1));
    }

    #[test]
    fn test_case_insensitive() {
        let nfa = DamLevNfa::new("hello", EditLimits::new(0), true);
        let matches = nfa.find_all("HELLO world", 0.8);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 5);
    }

    #[test]
    fn test_multiple_matches() {
        let nfa = DamLevNfa::new("ab", EditLimits::new(0), false);
        let matches = nfa.find_all("ab ab ab", 0.8);

        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_no_match() {
        let nfa = DamLevNfa::new("xyz", EditLimits::new(1), false);
        let matches = nfa.find_all("hello world", 0.8);

        assert!(matches.is_empty());
    }

    // --- Transposition tests ---

    #[test]
    fn test_transposition_simple() {
        // Pattern "ab" should match "ba" with 1 swap
        let nfa = DamLevNfa::new("ab", EditLimits::new(1), false);
        let matches = nfa.find_all("ba", 0.0);

        assert!(!matches.is_empty(), "Should find match for transposition");
        // Find the swap match (there might be other matches like deletion-only)
        let swap_match = matches.iter().find(|m| m.swaps == 1);
        assert!(
            swap_match.is_some(),
            "Should find match with 1 swap, got: {matches:?}"
        );
        let m = swap_match.unwrap();
        assert_eq!(m.substitutions, 0, "Should have 0 substitutions");
        assert_eq!(m.total_edits(), 1, "Total edits should be 1");
    }

    #[test]
    fn test_transposition_in_word() {
        // Pattern "teh" should match "the" with 1 swap (common typo)
        let nfa = DamLevNfa::new("the", EditLimits::new(1), false);
        let matches = nfa.find_all("teh", 0.0);

        assert!(!matches.is_empty(), "Should find match for 'teh' -> 'the'");
        let m = matches.iter().find(|m| m.swaps == 1);
        assert!(m.is_some(), "Should find match with 1 swap");
    }

    #[test]
    fn test_transposition_longer_word() {
        // Pattern "receive" should match "recieve" with 1 swap (common typo)
        let nfa = DamLevNfa::new("receive", EditLimits::new(1), false);
        let matches = nfa.find_all("recieve", 0.0);

        assert!(
            !matches.is_empty(),
            "Should find match for 'recieve' -> 'receive'"
        );
        // Check that we found a match with 1 swap (i and e swapped)
        let swap_match = matches.iter().find(|m| m.swaps == 1);
        assert!(
            swap_match.is_some(),
            "Should find match with 1 swap, got: {matches:?}"
        );
    }

    #[test]
    fn test_transposition_with_other_edits() {
        // "abcd" matching "badc" - two transpositions
        let nfa = DamLevNfa::new("abcd", EditLimits::new(2), false);
        let matches = nfa.find_all("badc", 0.0);

        assert!(!matches.is_empty(), "Should find match with 2 swaps");
        // Should find a match with 2 swaps
        let m = matches.iter().find(|m| m.total_edits() == 2);
        assert!(m.is_some(), "Should find match with 2 total edits");
    }

    #[test]
    fn test_transposition_not_same_char() {
        // Transposition should only work for different adjacent characters
        // "aa" cannot be swapped to "aa" (same characters)
        let nfa = DamLevNfa::new("aa", EditLimits::new(1), false);
        let matches = nfa.find_all("aa", 0.0);

        // Should find exact match with 0 edits
        assert!(!matches.is_empty());
        let exact = matches.iter().find(|m| m.total_edits() == 0);
        assert!(exact.is_some(), "Should find exact match");
    }

    #[test]
    fn test_transposition_vs_substitution() {
        // With swaps enabled, "ab" -> "ba" should find a swap match with 1 edit
        let nfa = DamLevNfa::new("ab", EditLimits::new(2), false);
        let matches = nfa.find_all("ba", 0.0);

        // Should find a swap match (1 edit) among the results
        let swap_match = matches
            .iter()
            .find(|m| m.swaps == 1 && m.total_edits() == 1);
        assert!(
            swap_match.is_some(),
            "Should find match with 1 swap, got: {matches:?}"
        );

        // The swap match should have better similarity than substitution matches
        let best_sim = matches
            .iter()
            .map(|m| m.similarity)
            .max_by(|a, b| a.partial_cmp(b).unwrap());
        let swap_sim = swap_match.unwrap().similarity;
        assert!(
            swap_sim >= best_sim.unwrap() - 0.01,
            "Swap match should have high similarity"
        );
    }

    #[test]
    fn test_transposition_case_insensitive() {
        // Case insensitive transposition
        let nfa = DamLevNfa::new("AB", EditLimits::new(1), true);
        let matches = nfa.find_all("ba", 0.0);

        assert!(
            !matches.is_empty(),
            "Should find case-insensitive transposition"
        );
        let m = matches.iter().find(|m| m.swaps == 1);
        assert!(m.is_some(), "Should find match with 1 swap");
    }
}

#[test]
fn test_find_with_candidates() {
    let nfa = DamLevNfa::new("quik", EditLimits::new(1), false);

    // Test 1: All positions as candidates (like find_all)
    let text = "The quick brown fox";
    let all_positions: FxHashSet<usize> = text.char_indices().map(|(i, _)| i).collect();
    let matches = nfa.find_all_with_candidates(text, 0.8, &all_positions);
    println!("All positions candidates: {matches:?}");

    // Test 2: Only positions 3, 4 as candidates
    let limited: FxHashSet<usize> = vec![3, 4].into_iter().collect();
    let matches2 = nfa.find_all_with_candidates(text, 0.8, &limited);
    println!("Limited candidates (3,4): {matches2:?}");

    // Test 3: Compare with find_all
    let matches3 = nfa.find_all(text, 0.8);
    println!("find_all: {matches3:?}");

    assert!(!matches.is_empty(), "Should find match with all positions");
    assert!(
        !matches2.is_empty(),
        "Should find match with limited positions"
    );
}
