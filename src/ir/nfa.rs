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
use crate::types::{FuzzyLimits, MinEdits};

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
            | State::GroupEntry { .. }
            | State::CaptureStart { .. }
            | State::CaptureEnd { .. }
            | State::Anchor { .. }
            | State::Lookahead { .. }
            | State::LookaheadLiteral { .. }
            | State::Lookbehind { .. }
            | State::LookbehindLiteral { .. }
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
        self.states.iter().any(|state| {
            matches!(
                state,
                State::Lookahead { .. } | State::LookaheadLiteral { .. }
            )
        })
    }

    /// Check if this NFA contains lookbehind assertions.
    #[must_use]
    pub fn has_lookbehind(&self) -> bool {
        self.states.iter().any(|state| {
            matches!(
                state,
                State::Lookbehind { .. } | State::LookbehindLiteral { .. }
            )
        })
    }

    /// Check if this NFA contains word boundary anchors.
    #[must_use]
    pub fn has_word_boundary(&self) -> bool {
        self.states.iter().any(|state| {
            if let State::Anchor { kind, .. } = state {
                matches!(
                    kind,
                    Anchor::WordBoundary
                        | Anchor::NotWordBoundary
                        | Anchor::WordStart
                        | Anchor::WordEnd
                )
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
            State::Split { branches, greedy } if *greedy && branches.len() >= 2 => {
                branches.iter().any(|&b| {
                    self.check_word_bounded_class(b, visited, seen_start_boundary, seen_class)
                })
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
                    {
                        let ch = class.to_first_char()?;
                        let mut new_literal = literal.clone();
                        new_literal.push(ch);
                        stack.push((*next, new_literal, count));
                    }
                }
                State::Split { branches, greedy }
                    if *greedy && branches.len() == 2 && count < max_count =>
                {
                    // Branch 0: continue repetition, Branch 1: exit
                    // Push exit first (lower priority), then continue
                    stack.push((branches[1], literal.clone(), 0));
                    stack.push((branches[0], literal.clone(), count + 1));
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

        // A genuine single character-class-plus (`\d+`, `[a-z]*`) has exactly ONE
        // `Char` state — the looping class. More than one means there is extra
        // consuming structure (e.g. `\.+\d`, `@+\d{2}`, `\d+\d+`), which this
        // fast path would mis-handle by scanning only one class's run.
        if self.count_char_states() != 1 {
            return false;
        }

        let mut visited = vec![false; self.states.len()];
        self.check_char_class_plus(self.start, &mut visited, false, 0)
    }

    /// Count the number of input-consuming `Char` states in the NFA.
    fn count_char_states(&self) -> usize {
        self.states
            .iter()
            .filter(|s| matches!(s, State::Char { .. }))
            .count()
    }

    /// Check if this NFA is a character class plus OR lazy plus: [charset]+ or [charset]+?
    /// Returns true for patterns like \d+, \d+?, \w+, \w+?, [a-z]+, [a-z]+?
    /// This is used for fast path optimization in `find()` and `find_iter()`
    #[must_use]
    pub fn is_char_class_plus_or_lazy(&self) -> bool {
        // Skip for complex NFAs to avoid stack overflow
        if self.states.len() > 50 {
            return false;
        }

        // Check for either greedy or lazy loop
        let has_greedy_loop = self.has_char_class_loop_with_greedy(true);
        let has_lazy_loop = self.has_char_class_loop_with_greedy(false);
        if !has_greedy_loop && !has_lazy_loop {
            return false;
        }

        // Exactly one Char state — a single looping class, no extra structure
        // (see `is_char_class_plus`).
        if self.count_char_states() != 1 {
            return false;
        }

        // Try greedy first, then lazy
        let mut visited = vec![false; self.states.len()];
        if has_greedy_loop
            && self.check_char_class_plus_with_greedy(self.start, &mut visited, false, 0, true)
        {
            return true;
        }

        let mut visited = vec![false; self.states.len()];
        if has_lazy_loop
            && self.check_char_class_plus_with_greedy(self.start, &mut visited, false, 0, false)
        {
            return true;
        }

        false
    }

    /// If the whole NFA is a single fuzzy character-class repetition with a
    /// genuine edit budget and no other structure — `(?:CLASS+){e<=k}`,
    /// `(?:CLASS*){i<=k}`, `(?:CLASS+){s<=k}`, … — return the character class.
    ///
    /// Used by `find()`'s safe 0-edit fast path: for such a pattern (which is
    /// unanchored, so the leftmost match always starts at position 0 once the
    /// budget is ≥1), if the first text char is in CLASS then the min-edit
    /// leftmost match is exactly the greedy 0-edit class run — identical to a
    /// plain `CLASS+`. Any leading-non-class case (where the budget is actually
    /// spent) is left to the general NFA, so this never changes results.
    ///
    /// Returns `None` for anything with extra structure (anchors, captures,
    /// lookarounds, literals, backrefs, recursion, handlers, a second class),
    /// a non-loop quantifier (`(?:\w){e<=1}` has no `+`), a zero budget, or an
    /// exclusive lower bound (`min_edits`, which the 0-edit run would violate).
    #[must_use]
    pub fn fuzzy_char_class_plus(&self) -> Option<HirClass> {
        if self.states.len() > 50 {
            return None;
        }

        let mut fuzzy_char_id: Option<StateId> = None;
        let mut class: Option<&HirClass> = None;
        for (id, state) in self.states.iter().enumerate() {
            match state {
                State::Accept | State::Epsilon { .. } | State::Split { .. } => {}
                State::FuzzyChar {
                    class: c,
                    limits,
                    min_edits,
                    ..
                } => {
                    // A single consuming class only.
                    if fuzzy_char_id.is_some() {
                        return None;
                    }
                    // Any exclusive lower bound (`{0<e<=k}`, `{1<=s<=1}`)
                    // requires ≥1 edit, so the 0-edit greedy run would be an
                    // invalid match.
                    if min_edits.is_some_and(|m| !m.is_empty()) {
                        return None;
                    }
                    // Require a genuine edit budget (≥1). A 0-budget or a
                    // cost-only constraint (limits None) is left to the NFA.
                    let total = limits.as_ref().map_or(0, |l| {
                        l.get_edits().unwrap_or_else(|| {
                            l.get_insertions()
                                .unwrap_or(0)
                                .saturating_add(l.get_deletions().unwrap_or(0))
                                .saturating_add(l.get_substitutions().unwrap_or(0))
                        })
                    });
                    if total == 0 {
                        return None;
                    }
                    fuzzy_char_id = Some(id);
                    class = Some(c);
                }
                // Any other state disqualifies the fast path.
                _ => return None,
            }
        }

        let fc = fuzzy_char_id?;
        // Require the `+`/`*` loop: the class state must be reachable from its
        // own successor via epsilon/split transitions. Without a loop the
        // pattern matches a single char and the greedy run would over-consume.
        if !self.class_state_loops(fc) {
            return None;
        }
        class.cloned()
    }

    /// Whether a consuming fuzzy state (FuzzyChar/FuzzyLiteral) may match zero
    /// characters, i.e. its repetition has minimum 0 (`*`, `?`, `{0,m}`). A
    /// fuzzy atom is zero-width capable when some Split that can reach it is
    /// itself reachable from the pattern start without passing through the
    /// atom — the quantifier can be skipped entirely — and that Split also has
    /// a branch that skips to a non-consuming terminal (`$`/Accept) without
    /// passing through the atom (which excludes alternation: every `a|b`
    /// branch passes through a consuming atom). This is what mrab models as
    /// pure insertions over the following text (e.g. `(?:b*){i<=1}$` on "d"
    /// matches (0,1) by inserting "d"). It excludes `+`, whose loop Split is
    /// only reachable through the atom itself.
    pub(crate) fn fuzzy_state_zero_width(&self, id: StateId) -> bool {
        let fuzzy = matches!(
            &self.states[id],
            State::FuzzyChar { .. } | State::FuzzyLiteral { .. }
        );
        if !fuzzy {
            return false;
        }
        let mut result = false;
        for (sid, s) in self.states.iter().enumerate() {
            if let State::Split { branches, .. } = s {
                let reach = branches.iter().any(|&b| self.state_reaches_eps(b, id));
                let rav = self.reachable_avoiding(sid, id);
                if reach && rav {
                    for &b in branches {
                        let mut visited = vec![false; self.states.len()];
                        let btend = self.branch_skips_to_end(b, id, &mut visited);
                        if btend {
                            result = true;
                        }
                    }
                }
            }
        }
        result
    }

    /// Whether from `from` a non-consuming terminal (`Accept` or an end
    /// anchor) is reachable without passing through `ignore` and without
    /// crossing any other consuming state or text-sensitive lookaround.
    fn branch_skips_to_end(&self, from: StateId, ignore: StateId, visited: &mut Vec<bool>) -> bool {
        if from == ignore || visited[from] {
            return false;
        }
        visited[from] = true;
        match &self.states[from] {
            State::Accept
            | State::Anchor {
                kind: Anchor::End, ..
            } => true,
            State::Char { .. }
            | State::FuzzyChar { .. }
            | State::FuzzyLiteral { .. }
            | State::Backreference { .. }
            | State::Lookahead { .. }
            | State::LookaheadLiteral { .. } => false,
            State::Epsilon { targets } => targets
                .iter()
                .any(|t| self.branch_skips_to_end(*t, ignore, visited)),
            State::Split { branches, .. } => branches
                .iter()
                .any(|t| self.branch_skips_to_end(*t, ignore, visited)),
            State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::ResetMatchStart { next }
            | State::GroupEntry { next, .. }
            | State::Anchor { next, .. }
            | State::Lookbehind { next, .. }
            | State::LookbehindLiteral { next, .. }
            | State::Handler { next, .. }
            | State::AtomicGroup { next, .. }
            | State::RecursivePattern { next, .. }
            | State::RecursiveGroup { next, .. }
            | State::RecursiveNamedGroup { next, .. } => {
                self.branch_skips_to_end(*next, ignore, visited)
            }
        }
    }

    /// Whether `from` can reach `target` through only epsilon/split
    /// transitions (the trivial zero-length path counts).
    fn state_reaches_eps(&self, from: StateId, target: StateId) -> bool {
        let mut stack = vec![from];
        let mut visited = vec![false; self.states.len()];
        while let Some(s) = stack.pop() {
            if s == target {
                return true;
            }
            if visited[s] {
                continue;
            }
            visited[s] = true;
            match &self.states[s] {
                State::Epsilon { targets } => stack.extend(targets.iter().copied()),
                State::Split { branches, .. } => stack.extend(branches.iter().copied()),
                _ => {}
            }
        }
        false
    }

    /// Whether `target` is reachable from the pattern start without passing
    /// through `avoid`.
    fn reachable_avoiding(&self, target: StateId, avoid: StateId) -> bool {
        if target == avoid {
            return false;
        }
        let mut stack = vec![self.start];
        let mut visited = vec![false; self.states.len()];
        while let Some(s) = stack.pop() {
            if s == target {
                return true;
            }
            if s == avoid || visited[s] {
                continue;
            }
            visited[s] = true;
            match &self.states[s] {
                State::Epsilon { targets } => stack.extend(targets.iter().copied()),
                State::Split { branches, .. } => stack.extend(branches.iter().copied()),
                State::Char { next, .. }
                | State::FuzzyChar { next, .. }
                | State::FuzzyLiteral { next, .. }
                | State::CaptureStart { next, .. }
                | State::CaptureEnd { next, .. }
                | State::GroupEntry { next, .. }
                | State::Anchor { next, .. }
                | State::Lookahead { next, .. }
                | State::LookaheadLiteral { next, .. }
                | State::Lookbehind { next, .. }
                | State::LookbehindLiteral { next, .. }
                | State::Backreference { next, .. }
                | State::AtomicGroup { next, .. }
                | State::RecursivePattern { next, .. }
                | State::RecursiveGroup { next, .. }
                | State::RecursiveNamedGroup { next, .. }
                | State::Handler { next, .. }
                | State::ResetMatchStart { next } => stack.push(*next),
                State::Accept => {}
            }
        }
        false
    }

    /// Whether `fc` (a consuming class state) sits on a repetition loop: its
    /// successor can reach `fc` again through only epsilon/split transitions.
    fn class_state_loops(&self, fc: StateId) -> bool {
        self.fuzzy_state_loops(fc)
    }

    /// Whether a consuming state (Char/FuzzyChar/FuzzyLiteral) sits on a
    /// repetition loop: its successor can reach it again through only
    /// epsilon/split transitions.
    fn fuzzy_state_loops(&self, id: StateId) -> bool {
        let start = match &self.states[id] {
            State::FuzzyChar { next, .. }
            | State::Char { next, .. }
            | State::FuzzyLiteral { next, .. } => *next,
            _ => return false,
        };
        let mut stack = vec![start];
        let mut visited = vec![false; self.states.len()];
        while let Some(s) = stack.pop() {
            if s == id {
                return true;
            }
            if visited[s] {
                continue;
            }
            visited[s] = true;
            match &self.states[s] {
                State::Epsilon { targets } => stack.extend(targets.iter().copied()),
                State::Split { branches, .. } => stack.extend(branches.iter().copied()),
                _ => {}
            }
        }
        false
    }

    /// Check if the NFA has a Split state that creates a loop (for + quantifier)
    /// This is used to distinguish \d+ from \d{3}
    fn has_char_class_loop(&self) -> bool {
        self.has_char_class_loop_with_greedy(true)
    }

    /// Check if the NFA has a Split state that creates a loop for either greedy or lazy + quantifier
    fn has_char_class_loop_with_greedy(&self, greedy: bool) -> bool {
        for (state_id, state) in self.states.iter().enumerate() {
            if let State::Split {
                branches,
                greedy: is_greedy,
            } = state
                && *is_greedy == greedy
                && branches.len() >= 2
            {
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
        self.check_char_class_plus_with_greedy(state_id, visited, seen_class, depth, true)
    }

    fn check_char_class_plus_with_greedy(
        &self,
        state_id: StateId,
        visited: &mut [bool],
        seen_class: bool,
        depth: usize,
        expect_greedy: bool,
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
                .any(|&t| self.check_char_class_plus_with_greedy(t, visited, seen_class, depth + 1, expect_greedy)),
            State::Char { class: _, next } => {
                self.check_char_class_plus_with_greedy(*next, visited, true, depth + 1, expect_greedy)
            }
            State::Split { branches, greedy }
                // For \d+ (one or more), there's a Split with a loop back to the Char state.
                // For \d{3} (exactly 3), there are NO Splits - just chained Char states.
                // For \d* (zero or more), there's a Split with a loop.
                //
                // We must check that there's an ACTUAL LOOP (Split with branch leading back to a visited state).
                // Without a loop, it's an exact repetition like {3} which should NOT use this fast path.

                if *greedy == expect_greedy && branches.len() >= 2 => {
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
                            .any(|&b| self.check_char_class_plus_with_greedy(b, visited, seen_class, depth + 1, expect_greedy))
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

        // The class+literal fast path (`find_all_class_plus_literal`) can only
        // account for plain class chars and plain literals. A FuzzyChar state
        // means the pattern has a fuzzy operation the fast path ignores, so it
        // must not fire (e.g. `(?:b|aba|bca)\d[^d]{e<=1}`).
        if self
            .states
            .iter()
            .any(|s| matches!(s, State::FuzzyChar { .. }))
        {
            return false;
        }

        // A named char class must be the body of a greedy repetition loop
        // (`a+`/`a*`) whose exit leads to a FuzzyLiteral that ends the match.
        // This is the only shape `find_all_class_plus_literal` can soundly
        // handle: it extends the match backwards over class chars, which is only
        // valid for an unbounded greedy loop. Bounded/lazy repetitions
        // (`a{1,3}`, `a+?`) and bare `a` without a loop must not be treated as
        // class-plus, or the fast path over-extends. Likewise, patterns that
        // merely *contain* a named class, a Split and a literal (e.g. an
        // alternation of literals followed by `\d`) are misclassified, and the
        // fast path emits bare literal spans that ignore the rest of the pattern.
        self.states.iter().enumerate().any(|(idx, state)| {
            let State::Char { class, next } = state else {
                return false;
            };
            if class.named.is_empty() {
                return false;
            }
            if let Some(State::Split {
                branches,
                greedy: true,
                ..
            }) = self.states.get(*next)
                && branches.contains(&idx)
                && branches
                    .iter()
                    .any(|&b| self.class_fed_literal_ends_at_accept(b))
            {
                return true;
            }
            false
        })
    }

    /// Whether `state` is a `FuzzyLiteral` whose `next` is the Accept state.
    fn class_fed_literal_ends_at_accept(&self, state: StateId) -> bool {
        match self.states.get(state) {
            Some(State::FuzzyLiteral { next, .. }) => {
                matches!(self.states.get(*next), Some(State::Accept))
            }
            _ => false,
        }
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

        // Every Char state must be a digit class: a genuine digit-sequence is
        // only digits and separators. A non-digit class between the digits and
        // the separator (e.g. `\d{1,3}?[a-z]\.`) is not this shape, and the
        // `find_digit_sequence_with_separator` scan would mishandle it.
        if digit_chars.len() != self.count_char_states() {
            return false;
        }

        // The pattern must START with a digit (the separator sits BETWEEN digit
        // groups). A leading separator (`\.\d{1,3}?` = `.` then digits) is not a
        // digit-sequence and the scan would mis-handle it.
        if !self.starts_with_digit_char() {
            return false;
        }

        // Must have a FuzzyLiteral (the separator).
        if !self
            .states
            .iter()
            .any(|s| matches!(s, State::FuzzyLiteral { .. }))
        {
            return false;
        }

        // The pattern must END with a digit too: in a genuine digit-sequence the
        // separator always sits BETWEEN digit groups, so every separator is
        // followed by more digits. A TRAILING separator (`\d{1,3}?\.` = digits
        // then `.` with nothing after) is not this shape, and the scan would
        // mis-handle it (it matched `".1"` on `".1 "`). Reject if any separator
        // can reach Accept without a digit `Char` after it.
        !self.any_separator_reaches_accept_zero_width()
    }

    /// Whether any `FuzzyLiteral` (separator) can reach `Accept` through
    /// zero-width transitions only — i.e. it can be the last consuming element
    /// (a trailing separator). Used to reject non-digit-sequence shapes.
    fn any_separator_reaches_accept_zero_width(&self) -> bool {
        self.states.iter().any(|state| {
            matches!(state, State::FuzzyLiteral { next, .. }
                if self.reaches_accept_zero_width(*next, &mut vec![false; self.states.len()]))
        })
    }

    /// Whether `Accept` is reachable from `state` without consuming any input
    /// (following only Epsilon/Anchor/Capture/Split zero-width transitions).
    /// Stops at any consuming state (`Char`/`FuzzyChar`/`FuzzyLiteral`/...).
    fn reaches_accept_zero_width(&self, state: StateId, visited: &mut [bool]) -> bool {
        if state >= self.states.len() || visited[state] {
            return false;
        }
        visited[state] = true;
        match &self.states[state] {
            State::Accept => true,
            State::Epsilon { targets } => targets
                .iter()
                .any(|&t| self.reaches_accept_zero_width(t, visited)),
            State::Split { branches, .. } => branches
                .iter()
                .any(|&b| self.reaches_accept_zero_width(b, visited)),
            State::Anchor { next, .. }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::ResetMatchStart { next } => self.reaches_accept_zero_width(*next, visited),
            _ => false,
        }
    }

    /// Whether the pattern's first input-consuming state is a digit-class `Char`.
    fn starts_with_digit_char(&self) -> bool {
        let mut visited = vec![false; self.states.len()];
        let mut current = self.start;
        loop {
            if current >= self.states.len() || visited[current] {
                return false;
            }
            visited[current] = true;
            match &self.states[current] {
                State::Char { class, .. } => {
                    return class
                        .named
                        .iter()
                        .any(|n| matches!(n, crate::parser::NamedClass::Digit))
                        || class
                            .ranges
                            .iter()
                            .any(|(start, end)| *start >= '0' && *end <= '9');
                }
                State::Epsilon { targets } if targets.len() == 1 => current = targets[0],
                State::Anchor { next, .. }
                | State::CaptureStart { next, .. }
                | State::CaptureEnd { next, .. } => current = *next,
                _ => return false,
            }
        }
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
            State::FuzzyLiteral { next, .. }
                if seen_start_boundary && !seen_literal && !seen_end_boundary =>
            {
                // We have start boundary, now seeing literal
                self.check_word_bounded_literal(
                    *next,
                    visited,
                    seen_start_boundary,
                    false,
                    true, // Literal seen
                )
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
            State::Split { branches, greedy } if *greedy => {
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
        let mut visited = vec![false; self.states.len()];
        self.check_greedy_dotstar_prefix(self.start, &mut visited)
    }

    /// Check if pattern starts with greedy .* followed by suffix.
    ///
    /// `visited` guards against cycles in the NFA (e.g. `.+.+`, whose `+` loops
    /// would otherwise recurse forever and overflow the stack).
    fn check_greedy_dotstar_prefix(&self, state_id: StateId, visited: &mut [bool]) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return false; // Reached Accept without finding suffix
        }

        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Epsilon { targets } => {
                // Follow epsilon transitions to find the real start of pattern
                targets
                    .iter()
                    .any(|&target| self.check_greedy_dotstar_prefix(target, visited))
            }
            State::Anchor {
                kind: crate::parser::Anchor::Start,
                next,
            } => self.check_greedy_dotstar_prefix(*next, visited),
            State::Char { class, next }
                // Check if this is `.` (any character)
                if class.named.iter().any(|n| {
                    matches!(
                        n,
                        crate::parser::NamedClass::Any
                            | crate::parser::NamedClass::AnyExceptNewline
                    )
                }) && !class.negated
                => {
                    // Found `.` - now check for a `*`/`+` that repeats THIS dot.
                    self.check_greedy_star_after_dot(*next, state_id, visited)
                }
            _ => false,
        }
    }

    /// Check for a greedy `*`/`+` that repeats the dot at `dot_state`.
    ///
    /// The Split must actually repeat the dot — one of its branches must lead
    /// back to `dot_state`. Otherwise the Split belongs to a *following* element
    /// (e.g. `.(?:,\d)*`, where `.` is a single dot and `*` applies to `,\d`),
    /// which is not a `.*SUFFIX` pattern.
    fn check_greedy_star_after_dot(
        &self,
        state_id: StateId,
        dot_state: StateId,
        visited: &mut [bool],
    ) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return false; // Reached Accept without finding suffix
        }

        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Epsilon { targets } => targets
                .iter()
                .any(|&target| self.check_greedy_star_after_dot(target, dot_state, visited)),
            State::Split { branches, greedy } if *greedy && branches.len() >= 2 => {
                // One branch must loop back to the dot (the `*`/`+` repeats it).
                if !branches.iter().any(|&b| self.can_reach_state(b, dot_state)) {
                    return false;
                }
                // The suffix is the branch that exits the loop (does not lead
                // back to the dot).
                let exit = branches
                    .iter()
                    .copied()
                    .find(|&b| !self.can_reach_state(b, dot_state))
                    .unwrap_or(branches[1]);
                self.check_suffix_after_star(exit, visited)
            }
            _ => false,
        }
    }

    /// Check that after the * we have a suffix pattern that leads to Accept
    fn check_suffix_after_star(&self, state_id: StateId, visited: &mut [bool]) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return true; // Empty match after * is valid (.* can match empty)
        }

        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Accept => true, // Reached Accept - suffix was empty
            State::Epsilon { targets } => {
                for &target in targets {
                    if self.check_suffix_after_star(target, visited) {
                        return true;
                    }
                }
                false
            }
            // FuzzyLiteral is a valid suffix (a fixed literal string).
            State::FuzzyLiteral { next, .. } => {
                // After FuzzyLiteral, must reach Accept via a pure literal.
                self.check_reaches_accept(*next, visited)
            }
            // A single literal character is a valid suffix element. A character
            // CLASS (`\d`, `.`, `[a-z]`, negated) is NOT: the fast path uses
            // `literals[0]` (a fixed string) as the whole suffix via `rfind`, so
            // any class/group after the literal would be ignored (e.g. `.+@.{2}`
            // would match on just `@`).
            State::Char { class, next } if Self::is_literal_char(class) => {
                self.check_reaches_accept(*next, visited)
            }
            _ => false,
        }
    }

    /// Whether a `Char` state matches exactly one fixed literal character (not a
    /// character class like `\d`, `.`, `[a-z]`, or a negated class).
    fn is_literal_char(class: &crate::ir::hir::HirClass) -> bool {
        class.chars.len() == 1
            && class.ranges.is_empty()
            && class.named.is_empty()
            && !class.negated
    }

    /// Whether `class` is a bare `.` (`Any` / `AnyExceptNewline`). Returns
    /// `Some` with `dotall` for a pure dot class, `None` for anything else.
    fn is_dot_class(class: &crate::ir::hir::HirClass) -> Option<bool> {
        if class.negated || !class.chars.is_empty() || !class.ranges.is_empty() {
            return None;
        }
        match class.named.as_slice() {
            [crate::parser::NamedClass::Any] => Some(true),
            [crate::parser::NamedClass::AnyExceptNewline] => Some(false),
            _ => None,
        }
    }

    /// Check that Accept is reachable from `state_id` through ZERO-WIDTH
    /// transitions only (epsilon).
    ///
    /// The greedy-prefix fast path treats `literals[0]` as the ENTIRE suffix, so
    /// the suffix must be exactly one literal unit (the `FuzzyLiteral` / single
    /// `Char` that `check_suffix_after_star` already matched) immediately
    /// followed by Accept. Any further consuming state — another `Char`
    /// (e.g. the `a{2}` in `.+aa{2}`, whose extra chars are NOT in `literals[0]`),
    /// a `FuzzyLiteral`, a `Split`, or an anchor the fast path can't honor —
    /// means the suffix is longer than `literals[0]`, so we reject and let the
    /// DFA/NFA handle it.
    fn check_reaches_accept(&self, state_id: StateId, visited: &mut [bool]) -> bool {
        if state_id >= self.states.len() {
            return false;
        }

        if state_id == 0 {
            return true; // Reached Accept
        }

        if visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &self.states[state_id] {
            State::Accept => true,
            State::Epsilon { targets } => targets
                .iter()
                .any(|&t| self.check_reaches_accept(t, visited)),
            _ => false,
        }
    }

    /// Shape info for the `LITERAL .* LITERAL` / `LITERAL .+ LITERAL` fast path
    /// (greedy or lazy middle). Such patterns can be matched with two literal
    /// searches (memmem for the prefix, forward/`rfind` for the suffix) instead
    /// of a per-byte DFA scan — the same idea as `.*SUFFIX`.
    #[must_use]
    pub fn literal_dotstar_suffix(&self) -> Option<PrefixDotStarSuffix> {
        // Leading non-fuzzy literal (prefix).
        let (prefix_index, after_prefix) = self.leading_fuzzy_literal(self.start)?;

        // Middle: either the Split directly (.*), or a dot then the Split (.+).
        let (split_id, min_chars) = {
            let id = self.follow_epsilons(after_prefix)?;
            match &self.states[id] {
                State::Split { branches, .. } if branches.len() == 2 => (id, 0),
                State::Char { class, next } if Self::is_dot_class(class).is_some() => {
                    let split = self.follow_epsilons(*next)?;
                    match &self.states[split] {
                        State::Split { branches, .. } if branches.len() == 2 => (split, 1),
                        _ => return None,
                    }
                }
                _ => return None,
            }
        };

        let State::Split { branches, greedy } = &self.states[split_id] else {
            return None;
        };
        let greedy = *greedy;
        if branches.len() != 2 {
            return None;
        }

        // The loop branch must be a `.` that actually loops back to the Split.
        let loop_branch = branches
            .iter()
            .copied()
            .find(|&b| self.can_reach_state(b, split_id))?;
        let exit_branch = branches
            .iter()
            .copied()
            .find(|&b| !self.can_reach_state(b, split_id))?;

        let dot_state = self.follow_epsilons(loop_branch)?;
        let (dot_next, dotall) = match &self.states[dot_state] {
            State::Char { class, next } => (Some(*next), Self::is_dot_class(class)?),
            _ => return None,
        };
        if !self.can_reach_state(dot_next?, split_id) {
            return None;
        }

        // Trailing literal (suffix) ending at Accept.
        let suffix_index = self.trailing_fuzzy_literal(exit_branch)?;

        Some(PrefixDotStarSuffix {
            prefix_index,
            suffix_index,
            greedy,
            min_chars,
            dotall,
        })
    }

    /// Follow a single-target epsilon chain from `id` to the next consuming or
    /// structural state. Returns `None` for multi-target epsilons (ambiguity) or
    /// out-of-range states.
    fn follow_epsilons(&self, mut id: StateId) -> Option<StateId> {
        let mut guard = 0;
        loop {
            let state = self.states.get(id)?;
            match state {
                State::Epsilon { targets } if targets.len() == 1 => {
                    id = targets[0];
                }
                _ => return Some(id),
            }
            guard += 1;
            if guard > self.states.len() {
                return None;
            }
        }
    }

    /// Walk from `start` through epsilons to a non-fuzzy `FuzzyLiteral`; returns
    /// its pattern index and `next` state.
    fn leading_fuzzy_literal(&self, mut id: StateId) -> Option<(usize, StateId)> {
        let mut guard = 0;
        loop {
            let state = self.states.get(id)?;
            match state {
                State::Epsilon { targets } if targets.len() == 1 => {
                    id = targets[0];
                }
                State::FuzzyLiteral {
                    pattern_index,
                    limits,
                    min_edits,
                    cost_constraint,
                    next,
                    ..
                } if limits.is_none() && min_edits.is_none() && cost_constraint.is_none() => {
                    return Some((*pattern_index, *next));
                }
                _ => return None,
            }
            guard += 1;
            if guard > self.states.len() {
                return None;
            }
        }
    }

    /// Walk from `state_id` through epsilons to a non-fuzzy `FuzzyLiteral` that
    /// reaches `Accept` through zero-width transitions only; returns its pattern
    /// index.
    fn trailing_fuzzy_literal(&self, mut id: StateId) -> Option<usize> {
        let mut guard = 0;
        loop {
            let state = self.states.get(id)?;
            match state {
                State::Epsilon { targets } if targets.len() == 1 => {
                    id = targets[0];
                }
                State::FuzzyLiteral {
                    pattern_index,
                    limits,
                    min_edits,
                    cost_constraint,
                    next,
                    ..
                } if limits.is_none() && min_edits.is_none() && cost_constraint.is_none() => {
                    let mut visited = vec![false; self.states.len()];
                    if self.check_reaches_accept(*next, &mut visited) {
                        return Some(*pattern_index);
                    }
                    return None;
                }
                _ => return None,
            }
            guard += 1;
            if guard > self.states.len() {
                return None;
            }
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
                // The first text char is guaranteed to be in `class` only if the
                // first char can be neither substituted (then any char matches)
                // nor deleted (then the first text char is the next element).
                // An insertion budget is fine: it only adds an alternative match
                // starting at the class char, so existence is preserved.
                let total_edits = limits.as_ref().map_or(0, |l| {
                    l.get_edits().unwrap_or_else(|| {
                        l.get_insertions()
                            .unwrap_or(0)
                            .saturating_add(l.get_deletions().unwrap_or(0))
                            .saturating_add(l.get_substitutions().unwrap_or(0))
                            .saturating_add(l.get_swaps().unwrap_or(0))
                    })
                });
                let subs = limits.as_ref().and_then(FuzzyLimits::get_substitutions);
                let dels = limits.as_ref().and_then(FuzzyLimits::get_deletions);
                let sub_possible = total_edits > 0 && subs != Some(0);
                let del_possible = total_edits > 0 && dels != Some(0);
                if sub_possible || del_possible {
                    None
                } else {
                    Some(class.clone())
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
                    // (could be more sophisticated but this handles common cases).
                    // `named` MUST be compared too: without it, a nullable named class
                    // like `\d*` (named=[Digit], empty chars/ranges) whose skip-branch
                    // leads to `.` (named=[AnyExceptNewline], also empty chars/ranges)
                    // was wrongly deemed equal, so `first_char_class` returned `\d` and
                    // the matcher quick-rejected every non-digit start — dropping valid
                    // matches like `(?:\d*){i<=1}.` on "," (\d* empty, `.` matches ",").
                    if branch_class.chars != first.chars
                        || branch_class.ranges != first.ranges
                        || branch_class.negated != first.negated
                        || branch_class.named != first.named
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
        // `on_stack` detects cycles (a state currently being explored on the DFS
        // path); `memo` caches each state's result so branches that CONVERGE on a
        // shared downstream state (e.g. the single `$` reached by both arms of
        // `(?:ab)?$`) are not misread as cycles. Conflating the two — a single
        // "visited" flag — made convergent paths return false, so `(?:ab)?$` and
        // friends were wrongly treated as not end-anchored.
        let mut on_stack = vec![false; self.states.len()];
        let mut memo = vec![None; self.states.len()];
        self.check_ends_with_end_anchor(self.start, &mut on_stack, &mut memo)
    }

    /// Recursive helper for `ends_with_end_anchor`.
    fn check_ends_with_end_anchor(
        &self,
        state_id: StateId,
        on_stack: &mut [bool],
        memo: &mut [Option<bool>],
    ) -> bool {
        if on_stack[state_id] {
            // Back-edge (loop, e.g. the `(?:ab)*` cycle). This is the neutral
            // element for the universal "all paths to Accept go through `$`"
            // conjunction: looping adds no NEW way to reach Accept that bypasses
            // `$`, and any finite bypassing path still yields false via a plain
            // `Accept`. Returning false here would wrongly reject end-anchored
            // loops like `(?:ab)*$`.
            return true;
        }
        if let Some(cached) = memo[state_id] {
            return cached;
        }
        on_stack[state_id] = true;

        #[allow(clippy::match_same_arms)]
        let result = match &self.states[state_id] {
            // Accept without going through End anchor
            State::Accept => false,

            // Non-consuming states - check successors
            State::Epsilon { targets } => {
                // All targets must end with anchor
                let targets = targets.clone();
                !targets.is_empty()
                    && targets
                        .iter()
                        .all(|&t| self.check_ends_with_end_anchor(t, on_stack, memo))
            }

            State::Split { branches, .. } => {
                // All branches must end with anchor
                let branches = branches.clone();
                !branches.is_empty()
                    && branches
                        .iter()
                        .all(|&b| self.check_ends_with_end_anchor(b, on_stack, memo))
            }

            // Check anchors
            State::Anchor {
                kind: Anchor::End,
                next,
            } => {
                // Found End anchor - check that the path continues to Accept
                let next = *next;
                self.path_reaches_accept(next, &mut vec![false; self.states.len()])
            }

            State::Anchor { next, .. } => {
                // Other anchor - continue
                let next = *next;
                self.check_ends_with_end_anchor(next, on_stack, memo)
            }

            // Consuming states - check successor
            State::Char { next, .. }
            | State::FuzzyChar { next, .. }
            | State::FuzzyLiteral { next, .. }
            | State::GroupEntry { next, .. }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::Backreference { next, .. }
            | State::ResetMatchStart { next }
            | State::Lookahead { next, .. }
            | State::LookaheadLiteral { next, .. }
            | State::Lookbehind { next, .. }
            | State::LookbehindLiteral { next, .. }
            | State::AtomicGroup { next, .. }
            | State::RecursivePattern { next, .. }
            | State::RecursiveGroup { next, .. }
            | State::RecursiveNamedGroup { next, .. }
            | State::Handler { next, .. } => {
                let next = *next;
                self.check_ends_with_end_anchor(next, on_stack, memo)
            }
        };

        on_stack[state_id] = false;
        memo[state_id] = Some(result);
        result
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
                    m + 1 + limits.as_ref().map_or(0, FuzzyLimits::insertion_capacity) as usize
                })
            }

            State::FuzzyLiteral { .. } => None, // Need bridge info for accurate length

            State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::GroupEntry { next, .. }
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

            State::Lookahead { next, .. }
            | State::LookaheadLiteral { next, .. }
            | State::Lookbehind { next, .. }
            | State::LookbehindLiteral { next, .. } => {
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
                // FuzzyChar can match 0 or more characters depending on edits:
                // - Deletion: 0 chars (pattern char skipped)
                // - Exact/Substitution: 1 char
                // - Insertions: consume extra text chars (bounded by the
                //   per-type insertion budget)
                let (next_min, next_max) =
                    self.length_range_state(*next, pattern_lengths, visited, memo);
                let max_edits = limits
                    .as_ref()
                    .and_then(FuzzyLimits::get_edits)
                    .unwrap_or(0) as usize;
                let insertions =
                    limits.as_ref().map_or(0, FuzzyLimits::insertion_capacity) as usize;
                // With deletion allowed, can consume 0 chars; otherwise 1 char
                let char_min = usize::from(max_edits == 0);
                (next_min + char_min, next_max.map(|m| m + 1 + insertions))
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
            | State::GroupEntry { next, .. }
            | State::Anchor { next, .. }
            | State::Lookahead { next, .. }
            | State::LookaheadLiteral { next, .. }
            | State::Lookbehind { next, .. }
            | State::LookbehindLiteral { next, .. }
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
        /// Minimum edits required (for exclusive lower bounds and
        /// per-operation ranges like `{1<=s<=1}`).
        min_edits: Option<MinEdits>,
        /// Cost constraint (optional).
        cost_constraint: Option<CostConstraint>,
        /// Restriction on characters usable for edits, e.g. `{i<=1:[ab]}`.
        edit_chars: Option<EditCharRestriction>,
        /// Group ID for shared budget tracking across multi-piece fuzzy groups.
        fuzzy_group_id: Option<usize>,
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
        /// Minimum edits required (for exclusive lower bounds like `{0<e<5}`
        /// and per-operation ranges like `{2<=i<=3}`).
        min_edits: Option<MinEdits>,
        /// Cost constraint (optional).
        cost_constraint: Option<CostConstraint>,
        /// Group ID for shared budget tracking across multi-piece fuzzy groups.
        fuzzy_group_id: Option<usize>,
        /// True for repeat-folded single-char repetitions (`(?:b{2})`): mrab
        /// gives such bodies no trailing-insertion alternative.
        repeat_fold: bool,
        /// Next state after matching.
        next: StateId,
    },

    /// Entry marker of a fuzzy group. Marks the group as entered on this
    /// thread so that accept-time `min_edits` lower bounds apply even when the
    /// group body can match empty via an epsilon path (`(?:c?){1<=e<=2}` on
    /// "cX" must reject the 0-edit empty accept; without the marker the
    /// epsilon path never records that the group was entered).
    GroupEntry {
        /// The fuzzy group id this marker opens.
        group_id: usize,
        /// Next state after entering the group.
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

    /// Optimized lookahead for simple exact literal (inline, no sub-NFA).
    LookaheadLiteral {
        /// True for positive lookahead, false for negative.
        positive: bool,
        /// Literal bytes to match.
        literal: Vec<u8>,
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

    /// Optimized lookbehind for simple exact literal (inline, no sub-NFA).
    LookbehindLiteral {
        /// True for positive lookbehind, false for negative.
        positive: bool,
        /// Literal bytes to match.
        literal: Vec<u8>,
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
        /// Max total edits allowed within the recursive sub-match (`None` = none).
        max_edits: Option<u8>,
    },

    /// Recursive numbered group - (?1), (?2), etc.
    RecursiveGroup {
        /// The capture group number to recurse into.
        group: usize,
        /// Next state after the recursive call.
        next: StateId,
        /// Max total edits allowed within the recursive sub-match.
        max_edits: Option<u8>,
    },

    /// Recursive named group - (?&name) or (?P>name)
    RecursiveNamedGroup {
        /// The name of the capture group to recurse into.
        name: String,
        /// Next state after the recursive call.
        next: StateId,
        /// Max total edits allowed within the recursive sub-match.
        max_edits: Option<u8>,
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
        min_edits: Option<MinEdits>,
        cost_constraint: Option<CostConstraint>,
        fuzzy_group_id: Option<usize>,
        next: StateId,
    ) -> Self {
        State::FuzzyLiteral {
            pattern_index,
            limits,
            min_edits,
            cost_constraint,
            fuzzy_group_id,
            repeat_fold: false,
            next,
        }
    }

    /// Create a fuzzy-group entry marker state.
    #[must_use]
    pub fn group_entry(group_id: usize, next: StateId) -> Self {
        State::GroupEntry { group_id, next }
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
            FuzzyBridge::new(&literals, None, None, false, false).map(Arc::new)
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
            | State::GroupEntry { next, .. }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. }
            | State::Anchor { next, .. }
            | State::Lookahead { next, .. }
            | State::LookaheadLiteral { next, .. }
            | State::Lookbehind { next, .. }
            | State::LookbehindLiteral { next, .. }
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
#[derive(Debug, Clone, PartialEq)]
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

/// Shape info for the `LITERAL .* LITERAL` / `LITERAL .+ LITERAL` fast path
/// (greedy or lazy middle). Such patterns can be matched with two literal
/// searches (memmem for the prefix, forward/`rfind` for the suffix) instead
/// of a per-byte DFA scan — the same idea as `.*SUFFIX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixDotStarSuffix {
    /// Pattern index (into `LiteralPattern` list) of the leading literal.
    pub prefix_index: usize,
    /// Pattern index (into `LiteralPattern` list) of the trailing literal.
    pub suffix_index: usize,
    /// Greedy (`.*`) vs lazy (`.*?`) middle.
    pub greedy: bool,
    /// Minimum chars the middle must consume: 0 for `.*`, 1 for `.+`.
    pub min_chars: usize,
    /// Whether `.` matches newlines (dot-all).
    pub dotall: bool,
}

/// A literal pattern extracted from the HIR for fuzzy matching.
#[derive(Debug, Clone)]
pub struct LiteralPattern {
    /// The literal text.
    pub text: String,
    /// Fuzzy limits for this pattern.
    pub limits: Option<FuzzyLimits>,
    /// Minimum edits required (for exclusive lower bounds like `{0<e<5}` and
    /// per-operation ranges like `{2<=i<=3}`).
    pub min_edits: Option<MinEdits>,
    /// Character class restriction for edits.
    /// If set, all edit characters must be from this class.
    pub edit_chars: Option<EditCharRestriction>,
}

impl LiteralPattern {
    /// Create a new literal pattern.
    #[must_use]
    pub fn new(text: String, limits: Option<FuzzyLimits>, min_edits: Option<MinEdits>) -> Self {
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
        min_edits: Option<MinEdits>,
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
