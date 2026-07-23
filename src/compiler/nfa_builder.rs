//! NFA construction from HIR using Thompson's construction.

use crate::ir::hir::HirClass;
use crate::ir::{CostConstraint, CostInfo, Hir, LiteralPattern, Nfa, NfaFragment, State, StateId};
use std::collections::HashMap;

/// Builder for constructing an NFA from HIR.
pub struct NfaBuilder {
    /// The NFA being built.
    nfa: Nfa,
    /// Literal patterns collected during building.
    literals: Vec<LiteralPattern>,
    /// Current literal pattern index.
    literal_index: usize,
    /// Group start/end states for recursion.
    /// Maps group index -> (`start_state`, `end_state`).
    group_states: Vec<(StateId, StateId)>,
    /// Named group states for recursion.
    named_group_states: HashMap<String, (StateId, StateId)>,
}

impl NfaBuilder {
    /// Create a new NFA builder.
    #[must_use]
    pub fn new() -> Self {
        NfaBuilder {
            nfa: Nfa::new(),
            literals: Vec::new(),
            literal_index: 0,
            group_states: Vec::new(),
            named_group_states: HashMap::new(),
        }
    }

    /// Check if HIR is a simple literal that can be inlined.
    /// Returns the literal bytes if simple (no fuzzy), None otherwise.
    fn get_simple_literal(hir: &Hir) -> Option<Vec<u8>> {
        match hir {
            Hir::Literal {
                text,
                limits,
                min_edits,
                edit_chars,
                ..
            } => {
                if limits.is_none() && min_edits.is_none() && edit_chars.is_none() {
                    let txt = text.as_str();
                    if txt.chars().all(|c: char| {
                        c.is_ascii_alphanumeric()
                            || c == '.'
                            || c == '-'
                            || c == '_'
                            || c == '@'
                            || c == ':'
                            || c == '/'
                    }) {
                        Some(txt.as_bytes().to_vec())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Hir::Char(c) => {
                // Single character - can be inlined if ASCII
                let mut bytes = [0u8; 4];
                let encoded = c.encode_utf8(&mut bytes);
                if encoded.len() == 1 && encoded.as_bytes()[0].is_ascii() {
                    Some(encoded.as_bytes().to_vec())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Build an NFA from HIR.
    #[must_use]
    pub fn build(mut self, hir: &Hir) -> (Nfa, Vec<LiteralPattern>) {
        // Create the accept state first (index 0)
        let accept = 0; // Already created in Nfa::new()

        // Build the NFA fragment for the HIR
        let fragment = self.build_fragment(hir);

        // Patch all end states to go to accept
        for end in fragment.ends {
            self.patch_to(end, accept);
        }

        // Set the start state
        self.nfa.start = fragment.start;

        // Transfer group states to the NFA
        self.nfa.group_states = std::mem::take(&mut self.group_states);
        self.nfa.named_group_states = std::mem::take(&mut self.named_group_states);

        (self.nfa, self.literals)
    }

    /// Build an NFA fragment for a HIR node.
    #[allow(clippy::too_many_lines)]
    fn build_fragment(&mut self, hir: &Hir) -> NfaFragment {
        match hir {
            Hir::Empty => {
                // Empty matches immediately - create an epsilon state
                let state = self.nfa.add_state(State::Epsilon { targets: vec![] });
                NfaFragment::single(state, state)
            }

            Hir::Literal {
                text,
                limits,
                min_edits,
                cost_info,
                edit_chars,
                fuzzy_group_id,
            } => {
                // Create a fuzzy literal state
                let pattern_index = self.literal_index;
                self.literal_index += 1;
                self.literals.push(LiteralPattern::with_edit_chars(
                    text.clone(),
                    limits.clone(),
                    *min_edits,
                    edit_chars.clone(),
                ));

                // Convert CostInfo to CostConstraint
                let cost_constraint = cost_info.as_ref().and_then(convert_cost_info);

                let state = self.nfa.add_state(State::FuzzyLiteral {
                    pattern_index,
                    limits: limits.clone(),
                    min_edits: *min_edits,
                    cost_constraint,
                    fuzzy_group_id: *fuzzy_group_id,
                    next: 0, // Will be patched
                });
                NfaFragment::single(state, state)
            }

            Hir::Char(ch) => {
                // Create a single-character class
                let mut class = HirClass::new(false);
                class.add_char(*ch);
                let state = self.nfa.add_state(State::Char {
                    class,
                    next: 0, // Will be patched
                });
                NfaFragment::single(state, state)
            }

            Hir::Class(class) => {
                let state = self.nfa.add_state(State::Char {
                    class: class.clone(),
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::FuzzyClass {
                class,
                limits,
                min_edits,
                cost_info,
                edit_chars,
                fuzzy_group_id,
            } => {
                // Convert CostInfo to CostConstraint
                let cost_constraint = cost_info.as_ref().and_then(convert_cost_info);

                let state = self.nfa.add_state(State::FuzzyChar {
                    class: class.clone(),
                    limits: limits.clone(),
                    min_edits: *min_edits,
                    cost_constraint,
                    edit_chars: edit_chars.clone(),
                    fuzzy_group_id: *fuzzy_group_id,
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::Concat(parts) => self.build_concat(parts),

            Hir::Alt(alts) => self.build_alternation(alts),

            Hir::Repeat {
                expr,
                min,
                max,
                greedy,
            } => self.build_repeat(expr, *min, *max, *greedy),

            Hir::Capture { index, name, expr } => {
                // Wrap the expression with capture start/end states
                let inner = self.build_fragment(expr);

                let cap_start = self.nfa.add_state(State::CaptureStart {
                    index: *index,
                    next: inner.start,
                });

                let cap_end = self.nfa.add_state(State::CaptureEnd {
                    index: *index,
                    next: 0, // Will be patched
                });

                // Patch inner ends to capture end
                for end in inner.ends {
                    self.patch_to(end, cap_end);
                }

                // Record group states for recursion
                // Ensure the group_states vec is large enough
                if self.group_states.len() < *index {
                    self.group_states.resize(*index, (0, 0));
                }
                self.group_states[*index - 1] = (cap_start, cap_end);

                // Record named group states if applicable
                if let Some(group_name) = name {
                    self.named_group_states
                        .insert(group_name.clone(), (cap_start, cap_end));
                }

                NfaFragment::single(cap_start, cap_end)
            }

            Hir::Anchor(kind) => {
                let state = self.nfa.add_state(State::Anchor {
                    kind: *kind,
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::Lookahead { positive, expr } => {
                if let Some(literal_bytes) = Self::get_simple_literal(expr) {
                    let state = self.nfa.add_state(State::LookaheadLiteral {
                        positive: *positive,
                        literal: literal_bytes,
                        next: 0,
                    });
                    return NfaFragment::single(state, state);
                }

                // Build sub-NFA for the assertion
                let sub_builder = NfaBuilder::new();
                let (sub_nfa, sub_literals) = sub_builder.build(expr);

                let state = self.nfa.add_state(State::Lookahead {
                    positive: *positive,
                    nfa: Box::new(sub_nfa),
                    literals: sub_literals,
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::Lookbehind { positive, expr } => {
                // Check if we can use the optimized literal form
                if let Some(literal_bytes) = Self::get_simple_literal(expr) {
                    let state = self.nfa.add_state(State::LookbehindLiteral {
                        positive: *positive,
                        literal: literal_bytes,
                        next: 0,
                    });
                    return NfaFragment::single(state, state);
                }

                let sub_builder = NfaBuilder::new();
                let (sub_nfa, sub_literals) = sub_builder.build(expr);

                // Use the factory function which pre-builds the FuzzyBridge
                let state = self.nfa.add_state(State::lookbehind(
                    *positive,
                    Box::new(sub_nfa),
                    sub_literals,
                    0, // Will be patched
                ));
                NfaFragment::single(state, state)
            }

            Hir::Backreference { group, limits } => {
                let state = self.nfa.add_state(State::Backreference {
                    group: *group,
                    limits: limits.clone(),
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::NamedList { name: _ } => {
                // NamedList will be expanded at runtime with the provided word list
                // For compilation, we treat it as empty
                // The actual matching will resolve the named list from the regex
                let state = self.nfa.add_state(State::Epsilon { targets: vec![] });
                NfaFragment::single(state, state)
            }

            Hir::ResetMatchStart => {
                // \K resets the match start position
                // Add a state that will track the reset position during matching
                let state = self.nfa.add_state(State::ResetMatchStart { next: 0 });
                NfaFragment::single(state, state)
            }

            Hir::AtomicGroup { expr } => {
                // Build the sub-NFA for the atomic group's expression
                let sub_builder = NfaBuilder::new();
                let (sub_nfa, _) = sub_builder.build(expr);

                // Create atomic group state
                let next = self.nfa.add_state(State::Accept); // Placeholder
                let atomic_state = self.nfa.add_state(State::AtomicGroup {
                    nfa: Box::new(sub_nfa),
                    next,
                });

                // Patch the atomic group's next to continue after it
                self.patch_to(next, atomic_state);

                NfaFragment::single(atomic_state, atomic_state)
            }

            Hir::RecursivePattern => {
                // (?R) - recursively match the entire pattern. Emit a real
                // recursion state; the backtracker performs the subroutine call
                // (`next` is patched to the continuation by the caller).
                let state = self.nfa.add_state(State::RecursivePattern { next: 0 });
                NfaFragment::single(state, state)
            }

            Hir::RecursiveGroup { group } => {
                // (?0) = whole pattern, (?1), (?2), … = a numbered capture group.
                let state = self.nfa.add_state(State::RecursiveGroup {
                    group: *group,
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::RecursiveNamedGroup { name } => {
                // (?&name) / (?P>name) - recursively match a named capture group.
                let state = self.nfa.add_state(State::RecursiveNamedGroup {
                    name: name.clone(),
                    next: 0,
                });
                NfaFragment::single(state, state)
            }

            Hir::Handler { name } => {
                // (?call:name) - invoke a custom handler
                let state = self.nfa.add_state(State::Handler {
                    name: name.clone(),
                    next: 0, // Will be patched by caller
                });
                NfaFragment::single(state, state)
            }
        }
    }

    /// Build a concatenation of HIR nodes.
    fn build_concat(&mut self, parts: &[Hir]) -> NfaFragment {
        if parts.is_empty() {
            return self.build_fragment(&Hir::Empty);
        }

        let mut fragments: Vec<_> = parts.iter().map(|p| self.build_fragment(p)).collect();

        // Chain fragments together
        let first = fragments.remove(0);
        let mut current = first;

        for next in fragments {
            // Patch current ends to next start
            for end in current.ends {
                self.patch_to(end, next.start);
            }
            current = NfaFragment::new(current.start, next.ends);
        }

        current
    }

    /// Build an alternation (a|b|c).
    fn build_alternation(&mut self, alts: &[Hir]) -> NfaFragment {
        if alts.is_empty() {
            return self.build_fragment(&Hir::Empty);
        }

        if alts.len() == 1 {
            return self.build_fragment(&alts[0]);
        }

        // Build all alternative fragments
        let fragments: Vec<_> = alts.iter().map(|a| self.build_fragment(a)).collect();

        // Create a split state that branches to all alternatives
        let branches: Vec<_> = fragments.iter().map(|f| f.start).collect();
        let split = self.nfa.add_state(State::Split {
            branches,
            greedy: true,
        });

        // Collect all end states
        let ends: Vec<_> = fragments.into_iter().flat_map(|f| f.ends).collect();

        NfaFragment::new(split, ends)
    }

    /// Build a repetition (*, +, ?, {n,m}).
    fn build_repeat(
        &mut self,
        expr: &Hir,
        min: usize,
        max: Option<usize>,
        greedy: bool,
    ) -> NfaFragment {
        match (min, max) {
            // a? - zero or one
            (0, Some(1)) => self.build_optional(expr, greedy),

            // a* - zero or more
            (0, None) => self.build_star(expr, greedy),

            // a+ - one or more
            (1, None) => self.build_plus(expr, greedy),

            // a{n} - exactly n
            (n, Some(m)) if n == m => self.build_exact(expr, n),

            // a{n,} - at least n
            (n, None) => self.build_at_least(expr, n, greedy),

            // a{n,m} - between n and m
            (n, Some(m)) => self.build_between(expr, n, m, greedy),
        }
    }

    /// Build optional: a?
    fn build_optional(&mut self, expr: &Hir, greedy: bool) -> NfaFragment {
        let inner = self.build_fragment(expr);

        // Create a split: either match or skip
        // For greedy: branches = [match, skip] - try match first
        // For non-greedy: branches = [match, skip] but greedy=false tells matcher to try skip first
        let branches = vec![inner.start];
        let split = self.nfa.add_state(State::Split { branches, greedy });

        // The split can also skip to the end (epsilon)
        // The split itself is an end state (the "skip" path)
        let mut ends = inner.ends.clone();
        ends.push(split);

        NfaFragment::new(split, ends)
    }

    /// Build star: a*
    fn build_star(&mut self, expr: &Hir, greedy: bool) -> NfaFragment {
        let inner = self.build_fragment(expr);

        // Create a split state
        // branches[0] = inner.start (try to match)
        // branches[1] = exit (will be patched later)
        // For greedy: try branches[0] first (match more)
        // For non-greedy: try branches[1] first (match less / exit early)
        let split = self.nfa.add_state(State::Split {
            branches: vec![inner.start],
            greedy,
        });

        // Patch inner ends to loop back to split
        for end in &inner.ends {
            self.patch_to(*end, split);
        }

        // Split is both start and end (can skip entirely)
        NfaFragment::new(split, vec![split])
    }

    /// Build plus: a+
    fn build_plus(&mut self, expr: &Hir, greedy: bool) -> NfaFragment {
        let inner = self.build_fragment(expr);

        // Create a split for the loop back
        // branches[0] = inner.start (loop back to match more)
        // branches[1] = exit (will be patched later)
        // For greedy: try branches[0] first (match more)
        // For non-greedy: try branches[1] first (exit early)
        let split = self.nfa.add_state(State::Split {
            branches: vec![inner.start],
            greedy,
        });

        // Patch inner ends to the split
        for end in &inner.ends {
            self.patch_to(*end, split);
        }

        // Start is inner start, end is the split
        NfaFragment::new(inner.start, vec![split])
    }

    /// Build exact repetition: a{n}
    fn build_exact(&mut self, expr: &Hir, n: usize) -> NfaFragment {
        if n == 0 {
            return self.build_fragment(&Hir::Empty);
        }

        // Chain n copies
        let mut fragments: Vec<_> = (0..n).map(|_| self.build_fragment(expr)).collect();

        let first = fragments.remove(0);
        let mut current = first;

        for next in fragments {
            for end in current.ends {
                self.patch_to(end, next.start);
            }
            current = NfaFragment::new(current.start, next.ends);
        }

        current
    }

    /// Build at-least repetition: a{n,}
    fn build_at_least(&mut self, expr: &Hir, n: usize, greedy: bool) -> NfaFragment {
        if n == 0 {
            return self.build_star(expr, greedy);
        }

        // Build n required copies, then a*
        let required = self.build_exact(expr, n);
        let star = self.build_star(expr, greedy);

        // Chain them
        for end in required.ends {
            self.patch_to(end, star.start);
        }

        NfaFragment::new(required.start, star.ends)
    }

    /// Build bounded repetition: a{n,m}
    fn build_between(&mut self, expr: &Hir, n: usize, m: usize, greedy: bool) -> NfaFragment {
        if n > m {
            return self.build_fragment(&Hir::Empty);
        }

        if n == m {
            return self.build_exact(expr, n);
        }

        // Build n required copies
        let mut current = if n > 0 {
            self.build_exact(expr, n)
        } else {
            self.build_fragment(&Hir::Empty)
        };

        // Build (m - n) optional copies
        let optional_count = m - n;
        for _ in 0..optional_count {
            let opt = self.build_optional(expr, greedy);

            // Chain current to optional
            for end in &current.ends {
                self.patch_to(*end, opt.start);
            }

            current = NfaFragment::new(current.start, opt.ends);
        }

        current
    }

    /// Patch a state to point to a target.
    fn patch_to(&mut self, state: StateId, target: StateId) {
        match &mut self.nfa.states[state] {
            State::Accept => {} // Can't patch accept
            State::Epsilon { targets } => {
                if targets.is_empty() {
                    targets.push(target);
                }
            }
            State::Char { next, .. }
            | State::FuzzyChar { next, .. }
            | State::FuzzyLiteral { next, .. }
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
            | State::Handler { next, .. }
            | State::ResetMatchStart { next } => *next = target,
            State::Split { branches, .. } => {
                // For split, we need to add the target as an option
                if !branches.contains(&target) {
                    branches.push(target);
                }
            }
        }
    }
}

impl Default for NfaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an NFA from HIR.
/// Convert `CostInfo` from HIR to `CostConstraint` for NFA.
fn convert_cost_info(cost_info: &CostInfo) -> Option<CostConstraint> {
    cost_info.max_cost.map(|max_cost| CostConstraint {
        insertion_cost: cost_info.insertion_cost.unwrap_or(0),
        deletion_cost: cost_info.deletion_cost.unwrap_or(0),
        substitution_cost: cost_info.substitution_cost.unwrap_or(0),
        transposition_cost: cost_info.transposition_cost.unwrap_or(0),
        max_cost,
    })
}

/// Build an NFA from the given HIR, returning the NFA and extracted literals.
#[must_use]
pub fn build_nfa(hir: &Hir) -> (Nfa, Vec<LiteralPattern>) {
    NfaBuilder::new().build(hir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower;
    use crate::parser::parse;

    #[test]
    fn test_build_literal() {
        let ast = parse("hello").unwrap();
        let hir = lower(&ast, 2);
        let (nfa, literals) = build_nfa(&hir);

        assert!(nfa.state_count() >= 2); // At least start and accept
        assert_eq!(literals.len(), 1);
        assert_eq!(literals[0].text, "hello");
    }

    #[test]
    fn test_build_alternation() {
        // Single chars don't become literals (they're Hir::Char, not Hir::Literal)
        let ast = parse("a|b").unwrap();
        let hir = lower(&ast, 0);
        let (nfa, _literals) = build_nfa(&hir);

        // Should have a split state for alternation
        assert!(nfa.states.iter().any(|s| matches!(s, State::Split { .. })));

        // Test with multi-char literals
        let ast = parse("hello|world").unwrap();
        let hir = lower(&ast, 2);
        let (nfa, literals) = build_nfa(&hir);

        assert!(nfa.states.iter().any(|s| matches!(s, State::Split { .. })));
        assert_eq!(literals.len(), 2);
    }

    #[test]
    fn test_build_quantifier() {
        // Single char quantified - becomes Char state with split
        let ast = parse("a+").unwrap();
        let hir = lower(&ast, 0);
        let (nfa, _literals) = build_nfa(&hir);

        // Should have a split for the loop
        assert!(nfa.states.iter().any(|s| matches!(s, State::Split { .. })));

        // Test with literal quantified
        let ast = parse("(hello)+").unwrap();
        let hir = lower(&ast, 2);
        let (nfa, literals) = build_nfa(&hir);

        assert!(!literals.is_empty());
        assert!(nfa.states.iter().any(|s| matches!(s, State::Split { .. })));
    }

    #[test]
    fn test_build_capture() {
        let ast = parse("(abc)").unwrap();
        let hir = lower(&ast, 0);
        let (nfa, _) = build_nfa(&hir);

        // Should have capture start and end
        assert!(
            nfa.states
                .iter()
                .any(|s| matches!(s, State::CaptureStart { .. }))
        );
        assert!(
            nfa.states
                .iter()
                .any(|s| matches!(s, State::CaptureEnd { .. }))
        );
    }

    #[test]
    fn test_build_char_class() {
        let ast = parse("[a-z]").unwrap();
        let hir = lower(&ast, 0);
        let (nfa, _) = build_nfa(&hir);

        assert!(nfa.states.iter().any(|s| matches!(s, State::Char { .. })));
    }

    #[test]
    fn test_build_anchor() {
        let ast = parse("^hello$").unwrap();
        let hir = lower(&ast, 0);
        let (nfa, _) = build_nfa(&hir);

        let anchor_count = nfa
            .states
            .iter()
            .filter(|s| matches!(s, State::Anchor { .. }))
            .count();
        assert_eq!(anchor_count, 2);
    }
}
