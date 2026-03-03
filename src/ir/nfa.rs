//! NFA (Non-deterministic Finite Automaton) representation.
//!
//! The NFA is used for matching regex patterns. It supports:
//! - Fuzzy literal matching via Levenshtein automata
//! - Exact character and class matching
//! - Epsilon transitions for quantifiers and alternation
//! - Capture groups
//! - Anchors and assertions

use std::sync::Arc;

use crate::engine::fuzzy_bridge::FuzzyBridge;
use crate::types::FuzzyLimits;

use super::hir::HirClass;
use crate::parser::Anchor;

/// Cost constraint for fuzzy matching.
/// Allows specifying custom costs for each edit operation type.
#[derive(Debug, Clone, Default)]
pub struct CostConstraint {
    /// Cost per insertion (default: 1).
    pub insertion_cost: u8,
    /// Cost per deletion (default: 1).
    pub deletion_cost: u8,
    /// Cost per substitution (default: 1).
    pub substitution_cost: u8,
    /// Cost per transposition (default: 1).
    pub transposition_cost: u8,
    /// Maximum total cost allowed.
    pub max_cost: u8,
}

impl CostConstraint {
    /// Check if the given edit counts satisfy this cost constraint.
    #[must_use]
    pub fn is_satisfied(
        &self,
        insertions: u8,
        deletions: u8,
        substitutions: u8,
        transpositions: u8,
    ) -> bool {
        let total_cost = (u16::from(insertions) * u16::from(self.insertion_cost))
            + (u16::from(deletions) * u16::from(self.deletion_cost))
            + (u16::from(substitutions) * u16::from(self.substitution_cost))
            + (u16::from(transpositions) * u16::from(self.transposition_cost));
        total_cost < u16::from(self.max_cost)
    }
}

/// State identifier in the NFA.
pub type StateId = usize;

/// Pattern index for fuzzy literals.
pub type PatternIndex = usize;

/// NFA representation for fuzzy regex.
#[derive(Debug, Clone)]
pub struct Nfa {
    /// All states in the NFA.
    pub states: Vec<State>,
    /// The start state.
    pub start: StateId,
    /// Group sub-NFAs for recursion support.
    /// Maps group index -> (`start_state`, `end_state`) for the group's content.
    pub group_states: Vec<(StateId, StateId)>,
    /// Named group state ranges for recursion.
    /// Maps group name -> (`start_state`, `end_state`).
    pub named_group_states: std::collections::HashMap<String, (StateId, StateId)>,
}

impl Nfa {
    /// Create a new NFA with a single start state.
    #[must_use]
    pub fn new() -> Self {
        Nfa {
            states: vec![State::Accept],
            start: 0,
            group_states: Vec::new(),
            named_group_states: std::collections::HashMap::new(),
        }
    }

    /// Check if this NFA is "simple" - just a single `FuzzyLiteral` leading to Accept.
    /// Simple NFAs don't need full NFA simulation; we can use Bitap result directly.
    ///
    /// Returns true if the NFA is:
    /// - Start → (optional Epsilon) → `FuzzyLiteral` → (optional Epsilon) → Accept
    /// - No captures, no `min_edits`, no `cost_constraint`
    #[must_use]
    pub fn is_simple_fuzzy_only(&self) -> bool {
        let mut visited = vec![false; self.states.len()];
        self.check_simple_fuzzy_only(self.start, &mut visited, false)
    }

    /// Check if this NFA is a "simple alternation" - a Split of `FuzzyLiterals` all leading to Accept.
    ///
    /// Returns true if the NFA is:
    /// - Start → Split { branches: [`FuzzyLiteral` → Accept, `FuzzyLiteral` → Accept, ...] }
    /// - No captures, no `min_edits`, no `cost_constraint` in any branch
    ///
    /// This allows using a fast multi-pattern Bitap search instead of full NFA simulation.
    #[must_use]
    pub fn is_simple_alternation(&self) -> bool {
        !self.get_alternation_pattern_indices().is_empty()
    }

    /// Get the pattern indices for a simple alternation.
    ///
    /// Returns a vector of pattern indices if the NFA is a simple alternation of `FuzzyLiterals`,
    /// or an empty vector if the NFA is not a simple alternation.
    #[must_use]
    pub fn get_alternation_pattern_indices(&self) -> Vec<PatternIndex> {
        // Follow through initial epsilon transitions
        let mut state_id = self.start;
        loop {
            match &self.states[state_id] {
                State::Epsilon { targets } if targets.len() == 1 => {
                    state_id = targets[0];
                }
                _ => break,
            }
        }

        // Must be a Split state with multiple branches
        let branches = match &self.states[state_id] {
            State::Split { branches, .. } if branches.len() >= 2 => branches,
            _ => return Vec::new(),
        };

        let mut pattern_indices = Vec::with_capacity(branches.len());

        for &branch_id in branches {
            // Each branch must be FuzzyLiteral → (epsilon*) → Accept
            // with no min_edits or cost_constraint
            if let Some(idx) = self.check_simple_alternation_branch(branch_id) {
                pattern_indices.push(idx);
            } else {
                return Vec::new(); // Not a simple alternation
            }
        }

        pattern_indices
    }

    /// Check if a branch is a simple `FuzzyLiteral` → Accept path.
    /// Returns the pattern index if valid, None otherwise.
    fn check_simple_alternation_branch(&self, mut state_id: StateId) -> Option<PatternIndex> {
        // Skip initial epsilon transitions
        loop {
            match &self.states[state_id] {
                State::Epsilon { targets } if targets.len() == 1 => {
                    state_id = targets[0];
                }
                _ => break,
            }
        }

        // Must be FuzzyLiteral with no constraints
        let (pattern_index, next) = match &self.states[state_id] {
            State::FuzzyLiteral {
                pattern_index,
                next,
                min_edits: None,
                cost_constraint: None,
                ..
            } => (*pattern_index, *next),
            _ => return None,
        };

        // Follow to Accept (through epsilon transitions)
        let mut state_id = next;
        loop {
            match &self.states[state_id] {
                State::Accept => return Some(pattern_index),
                State::Epsilon { targets } if targets.len() == 1 => {
                    state_id = targets[0];
                }
                _ => return None,
            }
        }
    }

    /// Recursive helper for `is_simple_fuzzy_only`.
    /// `seen_fuzzy` tracks whether we've already seen a `FuzzyLiteral`.
    fn check_simple_fuzzy_only(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_fuzzy: bool,
    ) -> bool {
        if visited[state_id] {
            return false; // Cycle detected - not simple
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Accept => seen_fuzzy, // Must have seen exactly one FuzzyLiteral

            State::Epsilon { targets } => {
                // All targets must be simple
                if targets.len() != 1 {
                    return false; // Multiple branches - not simple
                }
                self.check_simple_fuzzy_only(targets[0], visited, seen_fuzzy)
            }

            State::FuzzyLiteral {
                next,
                min_edits,
                cost_constraint,
                ..
            } => {
                // Only simple if no constraints and haven't seen another FuzzyLiteral
                if seen_fuzzy || min_edits.is_some() || cost_constraint.is_some() {
                    return false;
                }
                self.check_simple_fuzzy_only(*next, visited, true)
            }

            // Any other state type makes it not simple
            State::Char { .. }
            | State::FuzzyChar { .. }
            | State::CaptureStart { .. }
            | State::CaptureEnd { .. }
            | State::Anchor { .. }
            | State::Lookahead { .. }
            | State::Lookbehind { .. }
            | State::Backreference { .. }
            | State::Split { .. }
            | State::ResetMatchStart { .. }
            | State::AtomicGroup { .. }
            | State::RecursivePattern { .. }
            | State::RecursiveGroup { .. }
            | State::RecursiveNamedGroup { .. }
            | State::Handler { .. } => false,
        }
    }

    /// Add a new state and return its ID.
    pub fn add_state(&mut self, state: State) -> StateId {
        let id = self.states.len();
        self.states.push(state);
        id
    }

    /// Get a reference to a state.
    #[must_use]
    pub fn state(&self, id: StateId) -> &State {
        &self.states[id]
    }

    /// Get a mutable reference to a state.
    pub fn state_mut(&mut self, id: StateId) -> &mut State {
        &mut self.states[id]
    }

    /// Check if a state is accepting.
    #[must_use]
    pub fn is_accepting(&self, id: StateId) -> bool {
        matches!(self.states[id], State::Accept)
    }

    /// Get the total number of states.
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    /// Check if this NFA contains any lazy (non-greedy) quantifiers.
    ///
    /// Returns true if any Split state has greedy=false.
    /// Used to determine whether to prefer shorter matches.
    #[must_use]
    pub fn has_lazy_quantifiers(&self) -> bool {
        self.states
            .iter()
            .any(|state| matches!(state, State::Split { greedy: false, .. }))
    }

    /// Check if this NFA contains a `ResetMatchStart` state (\K).
    /// Used to skip DFA which doesn't properly handle \K reset.
    #[must_use]
    pub fn has_reset_match_start(&self) -> bool {
        self.states
            .iter()
            .any(|state| matches!(state, State::ResetMatchStart { .. }))
    }

    /// Check if this NFA contains any lookahead or lookbehind assertions.
    #[must_use]
    pub fn has_lookahead(&self) -> bool {
        self.states
            .iter()
            .any(|state| matches!(state, State::Lookahead { .. }))
    }

    /// Check if this NFA contains word boundary anchors.
    #[must_use]
    pub fn has_word_boundary(&self) -> bool {
        self.states.iter().any(|state| {
            if let State::Anchor { kind, .. } = state {
                matches!(kind, Anchor::WordBoundary | Anchor::NotWordBoundary)
            } else {
                false
            }
        })
    }

    /// Check if this NFA contains non-word boundary anchors (\B).
    #[must_use]
    pub fn has_not_word_boundary(&self) -> bool {
        self.states.iter().any(|state| {
            if let State::Anchor { kind, .. } = state {
                matches!(kind, Anchor::NotWordBoundary)
            } else {
                false
            }
        })
    }

    /// Check if this NFA contains word boundary anchors (not non-word boundaries).
    /// Used to determine if we can use the literal word boundary fast path.
    #[must_use]
    pub fn has_literal_word_boundary(&self) -> bool {
        self.states.iter().any(|state| {
            if let State::Anchor { kind, .. } = state {
                matches!(kind, Anchor::WordBoundary)
            } else {
                false
            }
        })
    }

    /// Check if this NFA contains any recursive patterns.
    /// Used to determine whether to use backtracking engine.
    #[must_use]
    pub fn has_recursion(&self) -> bool {
        self.states.iter().any(|state| {
            matches!(
                state,
                State::RecursivePattern { .. }
                    | State::RecursiveGroup { .. }
                    | State::RecursiveNamedGroup { .. }
            )
        })
    }

    /// Check if this NFA is a simple word-bounded literal pattern like `\bword\b`.
    ///
    /// Returns true if the pattern is essentially:
    /// - `\b` at start
    /// - A single `FuzzyLiteral`
    /// - `\b` at end
    ///
    /// This enables an optimization where we can find literal positions and
    /// filter by word boundary instead of full NFA simulation.
    #[must_use]
    pub fn is_word_bounded_literal(&self) -> bool {
        let mut visited = vec![false; self.states.len()];
        self.check_word_bounded_literal(self.start, &mut visited, false, false, false)
    }

    /// Check if this NFA is a word-bounded literal pattern like `\bword\b`.
    ///
    /// Returns true if the pattern is exactly:
    /// - `\b` at start
    /// - A single `FuzzyLiteral`
    /// - `\b` at end
    ///
    /// This enables an optimization where we can find literal positions and
    /// filter by word boundary instead of full NFA simulation.
    fn check_word_bounded_literal(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_start_boundary: bool,
        seen_end_boundary: bool,
        seen_literal: bool,
    ) -> bool {
        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Accept => {
                // Valid only if we saw BOTH start and end boundaries with literal in between
                seen_start_boundary && seen_end_boundary && seen_literal
            }
            State::Epsilon { targets } if targets.len() == 1 => self.check_word_bounded_literal(
                targets[0],
                visited,
                seen_start_boundary,
                seen_end_boundary,
                seen_literal,
            ),
            State::Anchor {
                kind: crate::parser::Anchor::WordBoundary,
                next,
            } => {
                if seen_literal && !seen_end_boundary {
                    // This is the second boundary (end boundary)
                    // Only valid if we already saw a start boundary before the literal
                    if seen_start_boundary {
                        self.check_word_bounded_literal(
                            *next,
                            visited,
                            seen_start_boundary,
                            true, // Now we've seen the end boundary
                            seen_literal,
                        )
                    } else {
                        false
                    }
                } else if !seen_start_boundary && !seen_literal && !seen_end_boundary {
                    // This is the first boundary (start boundary)
                    self.check_word_bounded_literal(
                        *next, visited, true, // Start boundary seen
                        false, false,
                    )
                } else {
                    false // Invalid state
                }
            }
            State::FuzzyLiteral { next, .. } => {
                if seen_start_boundary && !seen_literal && !seen_end_boundary {
                    // We have start boundary, now seeing literal
                    self.check_word_bounded_literal(
                        *next,
                        visited,
                        seen_start_boundary,
                        false,
                        true, // Literal seen
                    )
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Extract the first character class that must match for this NFA.
    ///
    /// This is used for quick rejection - if the first character doesn't match,
    /// we can skip NFA simulation entirely.
    ///
    /// Returns None if:
    /// - The pattern can match empty string
    /// - The pattern starts with an anchor
    /// - The first character is ambiguous (multiple branches with different first chars)
    #[must_use]
    pub fn first_char_class(&self) -> Option<HirClass> {
        let mut visited = vec![false; self.states.len()];
        self.first_char_class_from(self.start, &mut visited)
    }

    fn first_char_class_from(&self, state_id: StateId, visited: &mut [bool]) -> Option<HirClass> {
        if visited[state_id] {
            return None; // Cycle
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Epsilon { targets } => {
                if targets.len() == 1 {
                    self.first_char_class_from(targets[0], visited)
                } else {
                    None // Multiple paths - ambiguous
                }
            }
            State::Char { class, .. } => Some(class.clone()),
            State::FuzzyChar { class, limits, .. } => {
                // Only use as prefilter if no deletions allowed
                // (deletion would allow skipping the first char)
                let max_deletions = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_deletions)
                    .unwrap_or(0);
                if max_deletions == 0 {
                    Some(class.clone())
                } else {
                    None
                }
            }
            State::CaptureStart { next, .. } | State::CaptureEnd { next, .. } => {
                self.first_char_class_from(*next, visited)
            }
            State::Split { branches, .. } => {
                // Check if all branches have the same first char class
                if branches.is_empty() {
                    return None;
                }
                let first = self.first_char_class_from(branches[0], visited)?;
                for &branch in &branches[1..] {
                    let branch_class = self.first_char_class_from(branch, visited)?;
                    // For simplicity, only use if all branches have exactly the same class
                    // (could be more sophisticated but this handles common cases)
                    if branch_class.chars != first.chars
                        || branch_class.ranges != first.ranges
                        || branch_class.negated != first.negated
                    {
                        return None;
                    }
                }
                Some(first)
            }
            _ => None,
        }
    }

    /// Create a sub-NFA (used for lookahead/lookbehind).
    #[must_use]
    pub fn into_sub_nfa(self) -> Box<Nfa> {
        Box::new(self)
    }

    /// Check if this NFA ends with an End anchor (`$`).
    ///
    /// For patterns like `\.$`, this allows the matcher to search from the end
    /// of the text instead of scanning from the beginning.
    ///
    /// Returns true if all paths to Accept go through an End anchor.
    #[must_use]
    pub fn ends_with_end_anchor(&self) -> bool {
        let mut visited = vec![false; self.states.len()];
        self.check_ends_with_end_anchor(self.start, &mut visited)
    }

    /// Recursive helper for `ends_with_end_anchor`.
    fn check_ends_with_end_anchor(&self, state_id: StateId, visited: &mut [bool]) -> bool {
        if visited[state_id] {
            return false; // Cycle - assume false
        }
        visited[state_id] = true;

        #[allow(clippy::match_same_arms)]
        match &self.states[state_id] {
            // Accept without going through End anchor
            State::Accept => false,

            // Non-consuming states - check successors
            State::Epsilon { targets } => {
                // All targets must end with anchor
                !targets.is_empty()
                    && targets
                        .iter()
                        .all(|&t| self.check_ends_with_end_anchor(t, visited))
            }

            State::Split { branches, .. } => {
                // All branches must end with anchor
                !branches.is_empty()
                    && branches
                        .iter()
                        .all(|&b| self.check_ends_with_end_anchor(b, visited))
            }

            // Check anchors
            State::Anchor {
                kind: Anchor::End,
                next,
            } => {
                // Found End anchor - check that the path continues to Accept
                self.path_reaches_accept(*next, &mut vec![false; self.states.len()])
            }

            State::Anchor { next, .. } => {
                // Other anchor - continue
                self.check_ends_with_end_anchor(*next, visited)
            }

            // Consuming states - check successor
            State::Char { next, .. }
            | State::FuzzyChar { next, .. }
            | State::FuzzyLiteral { next, .. }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::Backreference { next, .. }
            | State::ResetMatchStart { next }
            | State::Lookahead { next, .. }
            | State::Lookbehind { next, .. }
            | State::AtomicGroup { next, .. }
            | State::RecursivePattern { next, .. }
            | State::RecursiveGroup { next, .. }
            | State::RecursiveNamedGroup { next, .. }
            | State::Handler { next, .. } => self.check_ends_with_end_anchor(*next, visited),
        }
    }

    /// Check if a path reaches Accept (for end anchor validation).
    fn path_reaches_accept(&self, state_id: StateId, visited: &mut [bool]) -> bool {
        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Accept => true,
            State::Epsilon { targets } => targets
                .iter()
                .any(|&t| self.path_reaches_accept(t, visited)),
            State::CaptureEnd { next, .. } => self.path_reaches_accept(*next, visited),
            _ => false, // Any consuming state after End anchor would be invalid
        }
    }

    /// Get the maximum match length for simple (non-fuzzy) patterns.
    ///
    /// Returns None if the pattern has unbounded length (e.g., `.*`, `+`) or
    /// contains `FuzzyLiteral` states that need bridge info.
    ///
    /// For end-anchored patterns, this helps limit the search range.
    #[must_use]
    pub fn max_simple_length(&self) -> Option<usize> {
        let mut visited = vec![false; self.states.len()];
        self.max_simple_length_from(self.start, &mut visited)
    }

    fn max_simple_length_from(&self, state_id: StateId, visited: &mut [bool]) -> Option<usize> {
        if visited[state_id] {
            return None; // Cycle means unbounded
        }
        visited[state_id] = true;

        #[allow(clippy::match_same_arms)]
        let result = match &self.states[state_id] {
            State::Accept => Some(0),

            State::Epsilon { targets } => {
                // Max across all targets
                let mut max = Some(0);
                for &target in targets {
                    let t_max = self.max_simple_length_from(target, visited)?;
                    max = max.map(|m| m.max(t_max));
                }
                max
            }

            State::Char { next, .. } => self.max_simple_length_from(*next, visited).map(|m| m + 1),

            State::FuzzyChar { next, limits, .. } => {
                // With fuzzy, the match length can vary
                // Max is still 1 char for the pattern position
                self.max_simple_length_from(*next, visited).map(|m| {
                    m + 1
                        + limits
                            .as_ref()
                            .and_then(FuzzyLimits::get_insertions)
                            .unwrap_or(0) as usize
                })
            }

            State::FuzzyLiteral { .. } => None, // Need bridge info for accurate length

            State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::Anchor { next, .. } => self.max_simple_length_from(*next, visited),

            State::Split { branches, .. } => {
                // Max across all branches
                let mut max = Some(0);
                for &branch in branches {
                    let b_max = self.max_simple_length_from(branch, visited)?;
                    max = max.map(|m| m.max(b_max));
                }
                max
            }

            State::Lookahead { next, .. } | State::Lookbehind { next, .. } => {
                // Assertions don't consume
                self.max_simple_length_from(*next, visited)
            }

            State::Backreference { .. } => None, // Unknown length

            State::AtomicGroup { .. } => None, // Unknown length

            State::ResetMatchStart { .. } => None, // \K - length becomes unknown after reset

            State::RecursivePattern { .. } => None, // Recursive - unknown length
            State::RecursiveGroup { .. } => None,   // Recursive - unknown length
            State::RecursiveNamedGroup { .. } => None, // Recursive - unknown length
            State::Handler { .. } => None,          // Handler - unknown length
        };

        visited[state_id] = false; // Reset for other paths
        result
    }

    /// Calculate the minimum and maximum possible match lengths for this NFA.
    ///
    /// For lookbehind assertions, this helps determine how far back to search.
    /// Returns (`min_length`, `max_length`) where `max_length` is None if unbounded.
    ///
    /// The `pattern_lengths` callback provides (`char_len`, `max_edits`) for `FuzzyLiteral` patterns.
    pub fn length_range<F>(&self, pattern_lengths: F) -> (usize, Option<usize>)
    where
        F: Fn(usize) -> Option<(usize, u8)>,
    {
        let mut visited = vec![false; self.states.len()];
        let mut memo: Vec<Option<(usize, Option<usize>)>> = vec![None; self.states.len()];
        self.length_range_state(self.start, &pattern_lengths, &mut visited, &mut memo)
    }

    /// Recursive helper for `length_range`.
    fn length_range_state<F>(
        &self,
        state_id: StateId,
        pattern_lengths: &F,
        visited: &mut [bool],
        memo: &mut [Option<(usize, Option<usize>)>],
    ) -> (usize, Option<usize>)
    where
        F: Fn(usize) -> Option<(usize, u8)>,
    {
        // Return cached result if available
        if let Some(result) = memo[state_id] {
            return result;
        }

        // Cycle detection - return (0, unbounded) for cycles
        if visited[state_id] {
            return (0, None);
        }
        visited[state_id] = true;

        let result = match &self.states[state_id] {
            State::Accept | State::ResetMatchStart { .. } => (0, Some(0)),

            State::Epsilon { targets } => {
                // Min/max across all targets
                let mut min = usize::MAX;
                let mut max: Option<usize> = Some(0);
                for &target in targets {
                    let (t_min, t_max) =
                        self.length_range_state(target, pattern_lengths, visited, memo);
                    min = min.min(t_min);
                    max = match (max, t_max) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        _ => None,
                    };
                }
                if min == usize::MAX {
                    min = 0;
                }
                (min, max)
            }

            State::Char { next, .. } => {
                let (next_min, next_max) =
                    self.length_range_state(*next, pattern_lengths, visited, memo);
                (next_min + 1, next_max.map(|m| m + 1))
            }

            State::FuzzyChar { next, limits, .. } => {
                // FuzzyChar can match 0-2 characters depending on edits:
                // - Deletion: 0 chars (pattern char skipped)
                // - Exact/Substitution: 1 char
                // - (Insertion handled elsewhere in text loop)
                let (next_min, next_max) =
                    self.length_range_state(*next, pattern_lengths, visited, memo);
                let max_edits = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_edits)
                    .unwrap_or(0) as usize;
                // With deletion allowed, can consume 0 chars; otherwise 1 char
                let char_min = usize::from(max_edits == 0);
                (next_min + char_min, next_max.map(|m| m + 1))
            }

            State::FuzzyLiteral {
                pattern_index,
                next,
                ..
            } => {
                let (next_min, next_max) =
                    self.length_range_state(*next, pattern_lengths, visited, memo);
                if let Some((pat_len, max_edits)) = pattern_lengths(*pattern_index) {
                    // With fuzzy matching, length can vary by edits:
                    // - Insertions add chars to match
                    // - Deletions remove chars from match
                    let edits = max_edits as usize;
                    let fuzzy_min = pat_len.saturating_sub(edits);
                    let fuzzy_max = pat_len + edits;
                    (next_min + fuzzy_min, next_max.map(|m| m + fuzzy_max))
                } else {
                    // Unknown pattern - assume arbitrary length
                    (next_min, None)
                }
            }

            State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::Anchor { next, .. }
            | State::Lookahead { next, .. }
            | State::Lookbehind { next, .. }
            | State::AtomicGroup { next, .. } => {
                self.length_range_state(*next, pattern_lengths, visited, memo)
            }

            State::Backreference { next, .. } => {
                // Backreferences have unknown length (depends on captured text)
                let _ = self.length_range_state(*next, pattern_lengths, visited, memo);
                (0, None)
            }

            State::Split { branches, .. } => {
                // Min/max across all branches
                let mut min = usize::MAX;
                let mut max: Option<usize> = Some(0);
                for &branch in branches {
                    let (b_min, b_max) =
                        self.length_range_state(branch, pattern_lengths, visited, memo);
                    min = min.min(b_min);
                    max = match (max, b_max) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        _ => None,
                    };
                }
                if min == usize::MAX {
                    min = 0;
                }
                (min, max)
            }

            State::RecursivePattern { .. }
            | State::RecursiveGroup { .. }
            | State::RecursiveNamedGroup { .. }
            | State::Handler { .. } => (0, None), // Unknown
        };

        visited[state_id] = false;
        memo[state_id] = Some(result);
        result
    }
}

impl Default for Nfa {
    fn default() -> Self {
        Self::new()
    }
}

/// A state in the NFA.
#[derive(Debug, Clone)]
pub enum State {
    /// Accept state - match succeeded.
    Accept,

    /// Epsilon transition - no input consumed.
    Epsilon {
        /// Target states (multiple for splits).
        targets: Vec<StateId>,
    },

    /// Match a single character from a class.
    Char {
        /// The character class to match.
        class: HirClass,
        /// Next state after matching.
        next: StateId,
    },

    /// Match a single character from a class with fuzzy matching support.
    /// Used for character classes inside fuzzy groups like `(?:[a-z])~1`.
    FuzzyChar {
        /// The character class to match.
        class: HirClass,
        /// Fuzzy matching limits (insertions, deletions, substitutions).
        limits: Option<FuzzyLimits>,
        /// Minimum edits required (for exclusive lower bounds).
        min_edits: Option<u8>,
        /// Cost constraint (optional).
        cost_constraint: Option<CostConstraint>,
        /// Next state after matching.
        next: StateId,
    },

    /// Match a literal string with fuzzy matching.
    /// Uses Levenshtein automata for the match.
    FuzzyLiteral {
        /// Index into the pre-built pattern list.
        pattern_index: PatternIndex,
        /// Per-pattern fuzzy limits.
        limits: Option<FuzzyLimits>,
        /// Minimum edits required (for exclusive lower bounds like `{0<e<5}`).
        min_edits: Option<u8>,
        /// Cost constraint (optional).
        cost_constraint: Option<CostConstraint>,
        /// Next state after matching.
        next: StateId,
    },

    /// Start of a capture group.
    CaptureStart {
        /// Capture group index (1-based).
        index: usize,
        /// Next state.
        next: StateId,
    },

    /// End of a capture group.
    CaptureEnd {
        /// Capture group index (1-based).
        index: usize,
        /// Next state.
        next: StateId,
    },

    /// Anchor assertion.
    Anchor {
        /// The type of anchor.
        kind: Anchor,
        /// Next state if anchor matches.
        next: StateId,
    },

    /// Lookahead assertion.
    Lookahead {
        /// True for positive lookahead, false for negative.
        positive: bool,
        /// Sub-NFA to evaluate.
        nfa: Box<Nfa>,
        /// Literal patterns used by the sub-NFA.
        literals: Vec<LiteralPattern>,
        /// Next state if assertion passes.
        next: StateId,
    },

    /// Lookbehind assertion.
    Lookbehind {
        /// True for positive lookbehind, false for negative.
        positive: bool,
        /// Sub-NFA to evaluate.
        nfa: Box<Nfa>,
        /// Literal patterns used by the sub-NFA.
        literals: Vec<LiteralPattern>,
        /// Pre-built `FuzzyBridge` for efficient matching (shared via Arc for Clone).
        bridge: Option<Arc<FuzzyBridge>>,
        /// Next state if assertion passes.
        next: StateId,
    },

    /// Backreference - match the same text as a capture group.
    Backreference {
        /// The capture group to reference.
        group: usize,
        /// Optional fuzzy limits for fuzzy backreference matching.
        limits: Option<FuzzyLimits>,
        /// Next state after matching.
        next: StateId,
    },

    /// Split state for alternation (prioritized).
    /// Tries branches in order for greedy/non-greedy semantics.
    Split {
        /// Branch states in priority order.
        branches: Vec<StateId>,
        /// Whether this split is greedy (try first branch first) or non-greedy (try last branch first).
        /// For quantifiers like *, +, ?, this determines match preference.
        greedy: bool,
    },

    /// Reset match start - \K
    /// Resets the match start position to the current position.
    /// Everything before \K is matched but excluded from the final match.
    ResetMatchStart {
        /// Next state after the reset.
        next: StateId,
    },

    /// Atomic group - (?>expr)
    /// Once matched, prevents backtracking within the group.
    AtomicGroup {
        /// The sub-NFA for the expression inside the group.
        nfa: Box<Nfa>,
        /// Next state after the atomic group.
        next: StateId,
    },

    /// Recursive pattern - (?R)
    /// Recursively matches the entire pattern.
    RecursivePattern {
        /// Next state after the recursive call.
        next: StateId,
    },

    /// Recursive numbered group - (?1), (?2), etc.
    RecursiveGroup {
        /// The capture group number to recurse into.
        group: usize,
        /// Next state after the recursive call.
        next: StateId,
    },

    /// Recursive named group - (?&name) or (?P>name)
    RecursiveNamedGroup {
        /// The name of the capture group to recurse into.
        name: String,
        /// Next state after the recursive call.
        next: StateId,
    },

    /// Custom handler invocation - (?call:name)
    /// Calls a custom handler function at this point in the match.
    Handler {
        /// The name of the handler to invoke.
        name: std::sync::Arc<str>,
        /// Next state after the handler returns.
        next: StateId,
    },
}

impl State {
    /// Create an epsilon transition to a single target.
    #[must_use]
    pub fn epsilon(target: StateId) -> Self {
        State::Epsilon {
            targets: vec![target],
        }
    }

    /// Create an epsilon transition to multiple targets.
    #[must_use]
    pub fn epsilon_multi(targets: Vec<StateId>) -> Self {
        State::Epsilon { targets }
    }

    /// Create a character matching state.
    #[must_use]
    pub fn char_match(class: HirClass, next: StateId) -> Self {
        State::Char { class, next }
    }

    /// Create a fuzzy literal state.
    #[must_use]
    pub fn fuzzy_literal(
        pattern_index: PatternIndex,
        limits: Option<FuzzyLimits>,
        min_edits: Option<u8>,
        cost_constraint: Option<CostConstraint>,
        next: StateId,
    ) -> Self {
        State::FuzzyLiteral {
            pattern_index,
            limits,
            min_edits,
            cost_constraint,
            next,
        }
    }

    /// Create a capture start state.
    #[must_use]
    pub fn capture_start(index: usize, next: StateId) -> Self {
        State::CaptureStart { index, next }
    }

    /// Create a capture end state.
    #[must_use]
    pub fn capture_end(index: usize, next: StateId) -> Self {
        State::CaptureEnd { index, next }
    }

    /// Create an anchor state.
    #[must_use]
    pub fn anchor(kind: Anchor, next: StateId) -> Self {
        State::Anchor { kind, next }
    }

    /// Create a split state. Defaults to greedy.
    #[must_use]
    pub fn split(branches: Vec<StateId>) -> Self {
        State::Split {
            branches,
            greedy: true,
        }
    }

    /// Create a lookahead state.
    #[must_use]
    pub fn lookahead(
        positive: bool,
        nfa: Box<Nfa>,
        literals: Vec<LiteralPattern>,
        next: StateId,
    ) -> Self {
        State::Lookahead {
            positive,
            nfa,
            literals,
            next,
        }
    }

    /// Create a lookbehind state with pre-built `FuzzyBridge`.
    pub fn lookbehind(
        positive: bool,
        nfa: Box<Nfa>,
        literals: Vec<LiteralPattern>,
        next: StateId,
    ) -> Self {
        // Pre-build the FuzzyBridge for efficient matching
        let bridge = if literals.is_empty() {
            None
        } else {
            FuzzyBridge::new(&literals, None, None, false).map(Arc::new)
        };
        State::Lookbehind {
            positive,
            nfa,
            literals,
            bridge,
            next,
        }
    }

    /// Create a backreference state.
    #[must_use]
    pub fn backreference(group: usize, limits: Option<FuzzyLimits>, next: StateId) -> Self {
        State::Backreference {
            group,
            limits,
            next,
        }
    }

    /// Get the next state(s) from this state.
    #[must_use]
    pub fn next_states(&self) -> Vec<StateId> {
        #[allow(clippy::match_same_arms)]
        match self {
            State::Accept => vec![],
            State::Epsilon { targets } => targets.clone(),
            State::Char { next, .. }
            | State::FuzzyChar { next, .. }
            | State::FuzzyLiteral { next, .. }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::Anchor { next, .. }
            | State::Lookahead { next, .. }
            | State::Lookbehind { next, .. }
            | State::Backreference { next, .. }
            | State::AtomicGroup { next, .. }
            | State::RecursivePattern { next, .. }
            | State::RecursiveGroup { next, .. }
            | State::RecursiveNamedGroup { next, .. }
            | State::Handler { next, .. } => vec![*next],
            State::Split { branches, .. } => branches.clone(),
            State::ResetMatchStart { .. } => vec![],
        }
    }
}

/// Fragment of an NFA being built (used during construction).
#[derive(Debug, Clone)]
pub struct NfaFragment {
    /// Entry state of the fragment.
    pub start: StateId,
    /// Exit states of the fragment (to be patched).
    pub ends: Vec<StateId>,
}

impl NfaFragment {
    /// Create a new fragment with given start and ends.
    #[must_use]
    pub fn new(start: StateId, ends: Vec<StateId>) -> Self {
        NfaFragment { start, ends }
    }

    /// Create a fragment with a single end state.
    #[must_use]
    pub fn single(start: StateId, end: StateId) -> Self {
        NfaFragment {
            start,
            ends: vec![end],
        }
    }
}

/// Character class restriction for fuzzy edits.
/// When set, edits (insertions, substitutions, etc.) must involve characters from this class.
#[derive(Debug, Clone)]
pub struct EditCharRestriction {
    /// Characters that are allowed in edits.
    pub chars: Vec<char>,
    /// Character ranges allowed in edits.
    pub ranges: Vec<(char, char)>,
}

impl EditCharRestriction {
    /// Create a new edit character restriction.
    #[must_use]
    pub fn new(chars: Vec<char>, ranges: Vec<(char, char)>) -> Self {
        EditCharRestriction { chars, ranges }
    }

    /// Check if a character is allowed by this restriction.
    #[must_use]
    pub fn allows(&self, ch: char) -> bool {
        self.chars.contains(&ch)
            || self
                .ranges
                .iter()
                .any(|&(start, end)| ch >= start && ch <= end)
    }
}

/// A literal pattern extracted from the HIR for fuzzy matching.
#[derive(Debug, Clone)]
pub struct LiteralPattern {
    /// The literal text.
    pub text: String,
    /// Fuzzy limits for this pattern.
    pub limits: Option<FuzzyLimits>,
    /// Minimum edits required (for exclusive lower bounds like `{0<e<5}`).
    pub min_edits: Option<u8>,
    /// Character class restriction for edits.
    /// If set, all edit characters must be from this class.
    pub edit_chars: Option<EditCharRestriction>,
}

impl LiteralPattern {
    /// Create a new literal pattern.
    #[must_use]
    pub fn new(text: String, limits: Option<FuzzyLimits>, min_edits: Option<u8>) -> Self {
        LiteralPattern {
            text,
            limits,
            min_edits,
            edit_chars: None,
        }
    }

    /// Create a new literal pattern with character class restriction.
    #[must_use]
    pub fn with_edit_chars(
        text: String,
        limits: Option<FuzzyLimits>,
        min_edits: Option<u8>,
        edit_chars: Option<EditCharRestriction>,
    ) -> Self {
        LiteralPattern {
            text,
            limits,
            min_edits,
            edit_chars,
        }
    }
}
