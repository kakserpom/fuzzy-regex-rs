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

    /// Check if this NFA contains lookbehind assertions.
    #[must_use]
    pub fn has_lookbehind(&self) -> bool {
        self.states
            .iter()
            .any(|state| matches!(state, State::Lookbehind { .. }))
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

    /// Check if this NFA contains character classes (like \d, \w, [a-z]).
    #[must_use]
    pub fn has_char_classes(&self) -> bool {
        self.states.iter().any(|state| {
            if let State::Char { class, .. } = state {
                !class.ranges.is_empty() || !class.named.is_empty() || !class.chars.is_empty()
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

    /// Check if this NFA is a word-bounded character class pattern like \b\w+\b.
    /// This enables a fast path that scans for word boundaries and character class.
    #[must_use]
    pub fn is_word_bounded_class(&self) -> bool {
        // Must have word boundary
        if !self.states.iter().any(|state| {
            if let State::Anchor { kind, .. } = state {
                matches!(kind, Anchor::WordBoundary)
            } else {
                false
            }
        }) {
            return false;
        }

        // Must have Split (for + quantifier)
        let has_split = self
            .states
            .iter()
            .any(|s| matches!(s, State::Split { greedy: true, .. }));
        if !has_split {
            return false;
        }

        // Check for word boundary -> char class -> + -> word boundary
        let mut visited = vec![false; self.states.len()];
        self.check_word_bounded_class(self.start, &mut visited, false, false)
    }

    /// Check if this NFA is a word-bounded character class with exact repetition: \b\w{4}\b.
    /// Returns Some((min, max)) if detected, None otherwise.
    /// This enables a fast path that scans for word boundaries with exact count.
    #[must_use]
    pub fn is_word_bounded_class_exact(&self) -> Option<(u32, u32)> {
        // Must have word boundary
        if !self.states.iter().any(|state| {
            if let State::Anchor { kind, .. } = state {
                matches!(kind, Anchor::WordBoundary)
            } else {
                false
            }
        }) {
            return None;
        }

        // Check for word boundary -> char class -> bounded quantifier -> word boundary
        let mut visited = vec![false; self.states.len()];
        self.check_word_bounded_class_exact(self.start, &mut visited, false, false, None)
    }

    fn check_word_bounded_class_exact(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_start_boundary: bool,
        seen_class: bool,
        quant: Option<(u32, u32)>,
    ) -> Option<(u32, u32)> {
        if state_id >= self.states.len() {
            return None;
        }

        if visited[state_id] {
            return None;
        }
        visited[state_id] = true;

        if state_id == 0 {
            // Reached accept - valid pattern with quantifier
            if seen_start_boundary && seen_class {
                return quant;
            }
            return None;
        }

        match &self.states[state_id] {
            State::Epsilon { targets } => {
                for &t in targets {
                    if let Some(q) = self.check_word_bounded_class_exact(
                        t,
                        visited,
                        seen_start_boundary,
                        seen_class,
                        quant,
                    ) {
                        return Some(q);
                    }
                }
                None
            }
            State::Anchor {
                kind: Anchor::WordBoundary,
                next,
            } => {
                if !seen_start_boundary {
                    self.check_word_bounded_class_exact(*next, visited, true, seen_class, quant)
                } else if !seen_class {
                    None
                } else {
                    self.check_word_bounded_class_exact(
                        *next,
                        visited,
                        seen_start_boundary,
                        seen_class,
                        quant,
                    )
                }
            }
            State::Char { class: _, next } => self.check_word_bounded_class_exact(
                *next,
                visited,
                seen_start_boundary,
                true,
                quant,
            ),
            // Handle bounded quantifier like {4} or {3,5}
            // This would be represented as a Split with specific bounds
            State::Split { branches, greedy } => {
                if *greedy && branches.len() == 2 {
                    // For bounded quantifier, check both branches
                    // Branch 0: continue with increased count, Branch 1: exit with final count
                    for &b in branches {
                        if let Some(q) = self.check_word_bounded_class_exact(
                            b,
                            visited,
                            seen_start_boundary,
                            seen_class,
                            quant,
                        ) {
                            return Some(q);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn check_word_bounded_class(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_start_boundary: bool,
        seen_class: bool,
    ) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        if state_id == 0 {
            // Reached accept - valid pattern
            return seen_start_boundary && seen_class;
        }

        match &self.states[state_id] {
            State::Epsilon { targets } => targets.iter().any(|&t| {
                self.check_word_bounded_class(t, visited, seen_start_boundary, seen_class)
            }),
            State::Anchor {
                kind: Anchor::WordBoundary,
                next,
            } => {
                if !seen_start_boundary {
                    self.check_word_bounded_class(*next, visited, true, seen_class)
                } else if !seen_class {
                    false
                } else {
                    self.check_word_bounded_class(*next, visited, seen_start_boundary, seen_class)
                }
            }
            State::Char { class: _, next } => {
                self.check_word_bounded_class(*next, visited, seen_start_boundary, true)
            }
            State::Split { branches, greedy } => {
                if *greedy && branches.len() >= 2 {
                    branches.iter().any(|&b| {
                        self.check_word_bounded_class(b, visited, seen_start_boundary, seen_class)
                    })
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check if this NFA is a fixed repetition of exact literal: (?:literal){N}
    /// Returns `Some(concatenated_literal)` if detected, None otherwise.
    /// Only optimized for simple bounded literal repetitions: (?:literal){N}
    /// Returns `Some(concatenated_literal)` if detected, None otherwise.
    /// E.g., (?:abc){3} -> "abcabcabc"
    #[must_use]
    pub fn as_fixed_repetition(&self) -> Option<String> {
        // Only handle simple NFAs (exact bounded repetitions)
        let state_count = self.states.len();
        if !(3..=50).contains(&state_count) {
            return None;
        }

        // Use iterative DFS with explicit stack
        let mut stack: Vec<(StateId, String, usize)> = vec![(self.start, String::new(), 0)];
        let mut visited = vec![false; state_count];
        let max_count = 5; // Only handle small counts

        while let Some((state_id, literal, count)) = stack.pop() {
            if state_id >= state_count {
                continue;
            }

            // Limit iterations
            if stack.len() > 100 {
                return None;
            }

            let visited_idx = state_count.min(state_id * (max_count + 1) + count);
            if visited_idx >= visited.len() || visited[visited_idx] {
                continue;
            }
            visited[visited_idx] = true;

            match &self.states[state_id] {
                State::Accept => {
                    if count > 0 && !literal.is_empty() {
                        return Some(literal.repeat(count));
                    }
                }
                State::Epsilon { targets } => {
                    for &target in targets {
                        stack.push((target, literal.clone(), count));
                    }
                }
                State::Char { class, next } => {
                    // Only handle single character literals (not ranges)
                    if let Some(ch) = class.to_first_char() {
                        let mut new_literal = literal.clone();
                        new_literal.push(ch);
                        stack.push((*next, new_literal, count));
                    } else {
                        return None; // Not a simple literal
                    }
                }
                State::Split { branches, greedy } => {
                    if *greedy && branches.len() == 2 && count < max_count {
                        // Branch 0: continue repetition, Branch 1: exit
                        // Push exit first (lower priority), then continue
                        stack.push((branches[1], literal.clone(), 0));
                        stack.push((branches[0], literal.clone(), count + 1));
                    } else {
                        return None; // Not a simple repetition
                    }
                }
                _ => return None, // Complex state
            }
        }

        None
    }

    /// Check if this NFA is a character class plus: [charset]+
    /// Returns true only for patterns like \d+, \w+, [a-z]+ (one or more)
    /// Returns false for exact repetitions like \d{3}, \d{5} (exactly N)
    /// Returns false for optional patterns like \d?, \d* (zero or one, zero or more)
    #[must_use]
    pub fn is_char_class_plus(&self) -> bool {
        // Skip for complex NFAs to avoid stack overflow
        if self.states.len() > 50 {
            return false;
        }

        // First, check if there's a Split with a loop in the NFA
        // This is necessary to distinguish \d+ (which has a loop) from \d{3} (which doesn't)
        let has_split_with_loop = self.has_char_class_loop();
        if !has_split_with_loop {
            return false;
        }

        let mut visited = vec![false; self.states.len()];
        self.check_char_class_plus(self.start, &mut visited, false, 0)
    }

    /// Check if the NFA has a Split state that creates a loop (for + quantifier)
    /// This is used to distinguish \d+ from \d{3}
    fn has_char_class_loop(&self) -> bool {
        for (state_id, state) in self.states.iter().enumerate() {
            if let State::Split { branches, greedy } = state
                && *greedy
                && branches.len() >= 2
            {
                // Check if any branch loops back to an earlier state in the path
                for &branch in branches {
                    if self.can_reach_state(branch, state_id) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if from `state_id` we can reach `target_state_id` (simple DFS)
    fn can_reach_state(&self, state_id: StateId, target_state_id: StateId) -> bool {
        let mut stack = vec![state_id];
        let mut visited = vec![false; self.states.len()];

        while let Some(s) = stack.pop() {
            if s >= self.states.len() || visited[s] {
                continue;
            }
            visited[s] = true;

            if s == target_state_id {
                return true;
            }

            match &self.states[s] {
                State::Epsilon { targets } => stack.extend(targets),
                State::Split { branches, .. } => stack.extend(branches),
                State::Char { next, .. }
                | State::CaptureStart { next, .. }
                | State::CaptureEnd { next, .. }
                | State::Anchor { next, .. } => stack.push(*next),
                _ => {}
            }
        }
        false
    }

    fn check_char_class_plus(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_class: bool,
        depth: usize,
    ) -> bool {
        const MAX_DEPTH: usize = 50;
        if depth > MAX_DEPTH {
            return false;
        }

        if state_id >= self.states.len() {
            return false;
        }

        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        if state_id == 0 {
            return seen_class;
        }

        match &self.states[state_id] {
            State::Accept => seen_class,
            State::Epsilon { targets } => targets
                .iter()
                .any(|&t| self.check_char_class_plus(t, visited, seen_class, depth + 1)),
            State::Char { class: _, next } => {
                self.check_char_class_plus(*next, visited, true, depth + 1)
            }
            State::Split { branches, greedy } => {
                // For \d+ (one or more), there's a Split with a loop back to the Char state.
                // For \d{3} (exactly 3), there are NO Splits - just chained Char states.
                // For \d* (zero or more), there's a Split with a loop.
                //
                // We must check that there's an ACTUAL LOOP (Split with branch leading back to a visited state).
                // Without a loop, it's an exact repetition like {3} which should NOT use this fast path.

                if *greedy && branches.len() >= 2 {
                    // Check if any branch creates a loop (leads back to a state we've already visited)
                    // For \d+, one branch leads back to the Char state which we visited earlier in this path
                    let has_loop = branches.iter().any(|&b| {
                        // Check if this branch can reach a state we've already visited in the current path
                        // (not counting states marked as visited in the outer array)
                        self.can_reach_visited_state(b, state_id, visited)
                    });

                    // Only return true if there's an actual loop (for + or *)
                    // If no loop, it's a bounded quantifier like {3} which has different semantics
                    if has_loop {
                        branches
                            .iter()
                            .any(|&b| self.check_char_class_plus(b, visited, seen_class, depth + 1))
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check if a state can reach any state that's already been visited in the current traversal.
    /// This is used to detect loops in the NFA (for + and * quantifiers).
    fn can_reach_visited_state(
        &self,
        state_id: StateId,
        current: StateId,
        visited: &[bool],
    ) -> bool {
        // Don't include current state in the check (that would be immediate self-loop which isn't the case here)
        let mut stack = vec![state_id];
        let mut local_visited = vec![false; self.states.len()];

        while let Some(s) = stack.pop() {
            if s >= self.states.len() {
                continue;
            }
            if local_visited[s] {
                continue;
            }
            local_visited[s] = true;

            // Check if this state is already visited in the outer traversal (indicating a loop back)
            if visited[s] && s != current {
                return true;
            }

            // Continue exploring
            match &self.states[s] {
                State::Epsilon { targets } => stack.extend(targets),
                State::Char { next, .. } => stack.push(*next),
                State::Split { branches, .. } => stack.extend(branches),
                _ => {}
            }
        }

        false
    }

    /// Get the type of character class used in a char class plus pattern.
    /// Returns Some("digit"), Some("word"), Some("whitespace"), or None for custom ranges.
    /// Also handles negated classes: \D returns `not_digit`, \W returns `not_word`, \S returns `not_whitespace`
    #[must_use]
    pub fn get_char_class_type(&self) -> Option<&'static str> {
        for state in &self.states {
            if let State::Char { class, .. } = state {
                for named in &class.named {
                    let base_type = match named {
                        crate::parser::NamedClass::Digit => "digit",
                        crate::parser::NamedClass::Word => "word",
                        crate::parser::NamedClass::Whitespace => "whitespace",
                        crate::parser::NamedClass::NotDigit => return Some("not_digit"),
                        crate::parser::NamedClass::NotWord => return Some("not_word"),
                        crate::parser::NamedClass::NotWhitespace => return Some("not_whitespace"),
                        _ => continue,
                    };
                    // If the class is negated (e.g., \D = [^\d]), return the "not_" version
                    return Some(if class.negated {
                        match base_type {
                            "digit" => "not_digit",
                            "word" => "not_word",
                            "whitespace" => "not_whitespace",
                            _ => base_type,
                        }
                    } else {
                        base_type
                    });
                }
            }
        }
        None
    }

    /// Check if this NFA is a character class plus followed by literal: \w+@, \d+\., \S+pattern enables a fast path
    /// This: find the literal first with memchr, then extend backwards with the class.
    /// Only matches patterns that have BOTH a named character class AND a `FuzzyLiteral` state.
    #[must_use]
    pub fn is_class_plus_with_literal(&self) -> bool {
        if self.states.len() > 30 {
            return false;
        }

        // Must have Split (for +) and Char states with named classes
        let has_split = self.states.iter().any(
            |s| matches!(s, State::Split { greedy: true, branches, .. } if branches.len() >= 2),
        );

        // Check for named character class, but exclude "." (Any/AnyExceptNewline)
        let has_named_char = self.states.iter().any(|s| {
            if let State::Char { class, .. } = s {
                class.named.iter().any(|n| {
                    !matches!(
                        n,
                        crate::parser::NamedClass::Any
                            | crate::parser::NamedClass::AnyExceptNewline
                    )
                })
            } else {
                false
            }
        });

        // Also check for FuzzyLiteral state - this is what represents the literal part
        let has_fuzzy_literal = self
            .states
            .iter()
            .any(|s| matches!(s, State::FuzzyLiteral { .. }));

        // Must have Split, named char class, AND a FuzzyLiteral (the literal part)
        has_split && has_named_char && has_fuzzy_literal
    }

    /// Check if this NFA is a digit sequence with separators: \d{4}-\d{2}-\d{2}
    /// Pattern: N digit chars, separator, N digit chars, separator, N digit chars
    #[must_use]
    pub fn is_digit_sequence_with_separator(&self) -> bool {
        if self.states.len() > 20 {
            return false;
        }

        // Count Char states with digit class
        let digit_chars: Vec<_> = self
            .states
            .iter()
            .filter_map(|s| {
                if let State::Char { class, next } = s {
                    // Check if it's a digit class
                    let is_digit = class
                        .named
                        .iter()
                        .any(|n| matches!(n, crate::parser::NamedClass::Digit))
                        || class
                            .ranges
                            .iter()
                            .any(|(start, end)| *start >= '0' && *end <= '9');
                    if is_digit {
                        return Some(next);
                    }
                }
                None
            })
            .collect();

        // Must have at least 2 digit Char states
        if digit_chars.len() < 2 {
            return false;
        }

        // Check for FuzzyLiteral (separator)
        let has_literal = self
            .states
            .iter()
            .any(|s| matches!(s, State::FuzzyLiteral { .. }));

        digit_chars.len() >= 2 && has_literal
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

    /// Check if this NFA is a pure greedy dot-star pattern (e.g., `.*` or `.*$`).
    ///
    /// Returns true if the pattern consists only of:
    /// - Optional start anchor (^)
    /// - Greedy .* (any character zero or more times)
    /// - Optional end anchor ($)
    ///
    /// This enables an optimization where we can immediately return a match
    /// without scanning the text.
    #[must_use]
    pub fn is_pure_greedy_dotstar(&self) -> bool {
        // Quick check: must have at least one Split state (for the *)
        let has_split = self.states.iter().any(|s| matches!(s, State::Split { .. }));
        if !has_split {
            return false;
        }

        let mut visited = vec![false; self.states.len()];
        self.check_pure_greedy_dotstar(self.start, &mut visited, false, false)
    }

    fn check_pure_greedy_dotstar(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_dotstar: bool,
        seen_end_anchor: bool,
    ) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return true; // Reached Accept - pattern matched
        }

        if visited[state_id] {
            // Cycle detected - for greedy dotstar, this is expected
            // If we've seen the dotstar, it's valid (looping)
            return seen_dotstar;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Accept => true,
            State::Epsilon { targets } => {
                for &target in targets {
                    if !self.check_pure_greedy_dotstar(
                        target,
                        visited,
                        seen_dotstar,
                        seen_end_anchor,
                    ) {
                        return false;
                    }
                }
                true
            }
            State::Anchor { kind, next } => match kind {
                crate::parser::Anchor::Start => {
                    self.check_pure_greedy_dotstar(*next, visited, seen_dotstar, seen_end_anchor)
                }
                crate::parser::Anchor::End => {
                    // End anchor - if we've seen it, we're done
                    // If seen_dotstar is false but we reach End, that's also valid (empty .*)
                    if seen_end_anchor {
                        return true;
                    }
                    // Continue from next state to find Accept
                    self.check_pure_greedy_dotstar(*next, visited, seen_dotstar, true)
                }
                _ => false,
            },
            State::Char { class, next } => {
                if class.named.iter().any(|n| {
                    matches!(
                        n,
                        crate::parser::NamedClass::Any
                            | crate::parser::NamedClass::AnyExceptNewline
                    )
                }) && !class.negated
                {
                    self.check_pure_greedy_dotstar(*next, visited, true, seen_end_anchor)
                } else {
                    false
                }
            }
            State::Split { branches, greedy } => {
                if *greedy {
                    for &branch in branches {
                        if !self.check_pure_greedy_dotstar(
                            branch,
                            visited,
                            seen_dotstar,
                            seen_end_anchor,
                        ) {
                            return false;
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check if this NFA is a greedy prefix pattern: `.*SUFFIX`
    ///
    /// Returns true if the pattern is:
    /// - Optional start anchor (^)
    /// - Greedy .* (any character zero or more times)
    /// - Suffix pattern (literal or fuzzy literal)
    ///
    /// This enables an optimization where we can find the suffix first (using reverse search),
    /// then .* automatically matches everything before it. This avoids O(n²) behavior
    /// where greedy .* tries many ending positions with fuzzy matching at each.
    #[must_use]
    pub fn is_greedy_prefix_with_suffix(&self) -> bool {
        // Quick check: must have at least one Split state (for the *)
        let has_split = self
            .states
            .iter()
            .any(|s| matches!(s, State::Split { greedy: true, .. }));
        if !has_split {
            return false;
        }

        // Check that the pattern starts with .* followed by suffix (not alternation)
        // Pattern must be: ^? . (any char) * (greedy) SUFFIX
        self.check_greedy_dotstar_prefix(self.start)
    }

    /// Check if pattern starts with greedy .* followed by suffix
    fn check_greedy_dotstar_prefix(&self, state_id: StateId) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return false; // Reached Accept without finding suffix
        }

        match &self.states[state_id] {
            State::Epsilon { targets } => {
                // Follow epsilon transitions to find the real start of pattern
                targets
                    .iter()
                    .any(|&target| self.check_greedy_dotstar_prefix(target))
            }
            State::Anchor {
                kind: crate::parser::Anchor::Start,
                next,
            } => self.check_greedy_dotstar_prefix(*next),
            State::Char { class, next } => {
                // Check if this is `.` (any character)
                if class.named.iter().any(|n| {
                    matches!(
                        n,
                        crate::parser::NamedClass::Any
                            | crate::parser::NamedClass::AnyExceptNewline
                    )
                }) && !class.negated
                {
                    // Found `.` - now check for * (Split) after it
                    self.check_greedy_star_after_dot(*next)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check for greedy * after the dot
    fn check_greedy_star_after_dot(&self, state_id: StateId) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return false; // Reached Accept without finding suffix
        }

        match &self.states[state_id] {
            State::Epsilon { targets } => targets
                .iter()
                .any(|&target| self.check_greedy_star_after_dot(target)),
            State::Split { branches, greedy } if *greedy && branches.len() >= 2 => {
                // Found greedy * - now check that one branch leads to suffix (not just Accept)
                // branches[0] = loop back to continue consuming
                // branches[1] = skip * and go to suffix
                self.check_suffix_after_star(branches[1])
            }
            _ => false,
        }
    }

    /// Check that after the * we have a suffix pattern that leads to Accept
    fn check_suffix_after_star(&self, state_id: StateId) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return true; // Empty match after * is valid (.* can match empty)
        }

        match &self.states[state_id] {
            State::Accept => true, // Reached Accept - suffix was empty
            State::Epsilon { targets } => {
                for &target in targets {
                    if self.check_suffix_after_star(target) {
                        return true;
                    }
                }
                false
            }
            // FuzzyLiteral is a valid suffix
            State::FuzzyLiteral { next, .. } => {
                // After FuzzyLiteral, must reach Accept
                self.check_reaches_accept(*next)
            }
            // Char (non-dot) is also a valid suffix (exact literal chars)
            State::Char { next, .. } => self.check_reaches_accept(*next),
            _ => false,
        }
    }

    /// Check if we can reach Accept from this state
    fn check_reaches_accept(&self, state_id: StateId) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return true; // Reached Accept
        }

        match &self.states[state_id] {
            State::Accept => true,
            State::Epsilon { targets } => {
                for &target in targets {
                    if self.check_reaches_accept(target) {
                        return true;
                    }
                }
                false
            }
            State::Char { next, .. } | State::FuzzyLiteral { next, .. } => {
                self.check_reaches_accept(*next)
            }
            State::Split { branches, greedy } => {
                // For greedy split, all branches must reach Accept for valid suffix
                if *greedy {
                    for &branch in branches {
                        if !self.check_reaches_accept(branch) {
                            return false;
                        }
                    }
                    true
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
