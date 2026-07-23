//! Backtracking regex engine for recursive patterns and advanced features.
//!
//! This engine uses backtracking instead of NFA simulation, which allows
//! proper support for recursive patterns like `(?R)`, `(?1)`, etc.

#![allow(clippy::too_many_lines, clippy::comparison_chain)]

use crate::engine::captures::CaptureState;
use crate::engine::fuzzy_bridge::FuzzyBridge;
use crate::engine::matcher::{EditCounts, MatchResult, MatcherConfig};
use crate::ir::nfa::{Nfa, State, StateId};

/// Configuration for backtracking engine.
#[derive(Debug, Clone)]
pub struct BacktrackConfig {
    /// Whether to prefer shorter matches (non-greedy behavior).
    pub prefer_shortest: bool,
    /// Similarity threshold for fuzzy matching (0.0 - 1.0).
    pub threshold: f32,
    /// Whether this is an unanchored match.
    pub unanchored: bool,
}

impl From<MatcherConfig> for BacktrackConfig {
    fn from(config: MatcherConfig) -> Self {
        BacktrackConfig {
            prefer_shortest: config.prefer_shortest,
            threshold: config.threshold,
            unanchored: config.unanchored,
        }
    }
}

/// A saved state for backtracking.
#[derive(Debug, Clone)]
struct BacktrackFrame {
    /// State to backtrack to.
    state: StateId,
    /// Position in text when this frame was saved.
    pos: usize,
    /// Capture state at this point.
    captures: CaptureState,
    /// Similarity score at this point.
    similarity: f32,
    /// Edit counts at this point.
    edits: EditCounts,
    /// Match start position.
    match_start: usize,
    /// Recursion depth at this point.
    recursion_depth: usize,
    /// Subroutine call stack at this point (for `(?R)`/`(?0)`/`(?1)`/`(?&name)`).
    call_stack: Vec<CallFrame>,
}

/// An in-progress recursive subroutine call (`(?R)`, `(?0)`, `(?1)`, `(?&name)`).
///
/// When a recursion state is reached, the matcher jumps to the referenced
/// subpattern's start and pushes one of these; when it reaches the subpattern's
/// `end_state` it pops and resumes at `return_state`. This keeps recursion part
/// of the single backtracking search, so choices made inside a subroutine can be
/// revisited when a later part of the match fails (unlike a nested call).
#[derive(Clone, Debug)]
struct CallFrame {
    /// State that signals the subroutine has finished (accept for `(?R)`/`(?0)`,
    /// the group's capture-end for `(?1)`/`(?&name)`).
    end_state: StateId,
    /// State to resume at after the subroutine returns.
    return_state: StateId,
    /// Text position where the call was made — used to detect left recursion
    /// (a call to the same subpattern at the same position that makes no
    /// progress), which is failed rather than looped forever.
    start_pos: usize,
}

/// Backtracking regex matcher.
pub struct BacktrackMatcher<'a> {
    /// The NFA to execute.
    nfa: &'a Nfa,
    /// Bridge to fuzzy matching.
    fuzzy_bridge: Option<&'a FuzzyBridge>,
    /// Capture count.
    capture_count: usize,
    /// Configuration.
    config: BacktrackConfig,
    /// Maximum recursion depth to prevent stack overflow.
    max_recursion: usize,
}

impl<'a> BacktrackMatcher<'a> {
    /// Create a new backtracking matcher.
    #[must_use]
    pub fn new(
        nfa: &'a Nfa,
        fuzzy_bridge: Option<&'a FuzzyBridge>,
        capture_count: usize,
        config: BacktrackConfig,
    ) -> Self {
        BacktrackMatcher {
            nfa,
            fuzzy_bridge,
            capture_count,
            config,
            max_recursion: 1000,
        }
    }

    /// Find a match in the text.
    #[must_use]
    pub fn find(&self, text: &str) -> Option<MatchResult> {
        self.find_at(text, 0)
    }

    /// Find a match starting at a specific position.
    #[must_use]
    pub fn find_at(&self, text: &str, start: usize) -> Option<MatchResult> {
        let text_bytes = text.as_bytes();

        if self.config.unanchored {
            // Try each starting position
            for pos in start..text.len() {
                if let Some(result) = self.try_match(text, text_bytes, pos, pos, 0) {
                    return Some(result);
                }
            }
            None
        } else {
            // Anchored match - must start at exact position
            self.try_match(text, text_bytes, start, start, 0)
        }
    }

    /// Try to match from a specific position.
    fn try_match(
        &self,
        text: &str,
        text_bytes: &[u8],
        mut pos: usize,
        mut match_start: usize,
        mut depth: usize,
    ) -> Option<MatchResult> {
        // Initialize captures
        let mut captures = CaptureState::new(self.capture_count);
        let mut similarity = 1.0f32;
        let mut edits = EditCounts::default();

        // Backtracking stack
        let mut backtrack_stack: Vec<BacktrackFrame> = Vec::new();
        // Subroutine call stack for recursion (`(?R)`/`(?0)`/`(?1)`/`(?&name)`).
        let mut call_stack: Vec<CallFrame> = Vec::new();

        // Current state
        let mut state = self.nfa.start;

        loop {
            // Subroutine return: if we are inside a recursive call and have
            // reached that call's end state (the accept state for `(?R)`/`(?0)`,
            // or the group's capture-end for `(?1)`/`(?&name)`), pop the call and
            // resume at the return state instead of accepting/continuing.
            if let Some(frame) = call_stack.last()
                && state == frame.end_state
            {
                let return_state = frame.return_state;
                call_stack.pop();
                state = return_state;
                continue;
            }

            // Check for end of input (only a real match when not inside a call).
            if state == 0 {
                // Accept state - found a match
                return Some(MatchResult {
                    start: match_start,
                    end: pos,
                    similarity,
                    edits,
                    captures,
                });
            }

            let state_data = &self.nfa.states[state];

            match state_data {
                State::Accept => {
                    // Found a match!
                    return Some(MatchResult {
                        start: match_start,
                        end: pos,
                        similarity,
                        edits,
                        captures,
                    });
                }

                State::Epsilon { targets } => {
                    // Epsilon transition - no input consumed
                    if targets.len() == 1 {
                        state = targets[0];
                    } else if targets.len() > 1 {
                        // Choice point - save state for backtracking
                        // Save current state (will continue with first target)
                        state = targets[0];
                        // Save other targets for backtracking
                        for &target in &targets[1..] {
                            backtrack_stack.push(BacktrackFrame {
                                state: target,
                                pos,
                                captures: captures.clone(),
                                similarity,
                                edits: edits.clone(),
                                match_start,
                                recursion_depth: depth,
                                call_stack: call_stack.clone(),
                            });
                        }
                    } else {
                        // Empty epsilon - dead state
                        // Try to backtrack
                        {
                            let frame = backtrack_stack.pop()?;
                            state = frame.state;
                            pos = frame.pos;
                            captures = frame.captures;
                            similarity = frame.similarity;
                            edits = frame.edits;
                            match_start = frame.match_start;
                            depth = frame.recursion_depth;
                            call_stack = frame.call_stack;
                        }
                    }
                }

                State::Char { class, next } => {
                    // Try to match a character
                    if let Some(ch) = text[pos..].chars().next() {
                        if class.matches(ch) {
                            pos += ch.len_utf8();
                            state = *next;
                        } else {
                            // Try to backtrack
                            {
                                let frame = backtrack_stack.pop()?;
                                state = frame.state;
                                pos = frame.pos;
                                captures = frame.captures;
                                similarity = frame.similarity;
                                edits = frame.edits;
                                match_start = frame.match_start;
                                depth = frame.recursion_depth;
                                call_stack = frame.call_stack;
                            }
                        }
                    } else {
                        // End of input - try to backtrack
                        {
                            let frame = backtrack_stack.pop()?;
                            state = frame.state;
                            pos = frame.pos;
                            captures = frame.captures;
                            similarity = frame.similarity;
                            edits = frame.edits;
                            match_start = frame.match_start;
                            depth = frame.recursion_depth;
                            call_stack = frame.call_stack;
                        }
                    }
                }

                State::FuzzyChar {
                    class,
                    limits: _,
                    min_edits: _,
                    cost_constraint: _,
                    edit_chars: _,
                    fuzzy_group_id: _,
                    next,
                } => {
                    // For now, treat fuzzy char as exact match
                    if let Some(ch) = text[pos..].chars().next() {
                        if class.matches(ch) {
                            pos += ch.len_utf8();
                            state = *next;
                        } else {
                            let frame = backtrack_stack.pop()?;
                            state = frame.state;
                            pos = frame.pos;
                            captures = frame.captures;
                            similarity = frame.similarity;
                            edits = frame.edits;
                            match_start = frame.match_start;
                            call_stack = frame.call_stack;
                        }
                    } else {
                        let frame = backtrack_stack.pop()?;
                        state = frame.state;
                        pos = frame.pos;
                        captures = frame.captures;
                        similarity = frame.similarity;
                        edits = frame.edits;
                        match_start = frame.match_start;
                        call_stack = frame.call_stack;
                    }
                }

                State::FuzzyLiteral {
                    pattern_index,
                    next,
                    ..
                } => {
                    // Try to match a literal string
                    if let Some(bridge) = self.fuzzy_bridge {
                        if let Some(pattern) = bridge.pattern_text(*pattern_index) {
                            let remaining = &text[pos..];
                            if remaining.starts_with(pattern) {
                                // Exact match
                                pos += pattern.len();
                                state = *next;
                            } else {
                                // Try fuzzy match
                                // For simplicity, treat as no match for now
                                {
                                    let frame = backtrack_stack.pop()?;
                                    state = frame.state;
                                    pos = frame.pos;
                                    captures = frame.captures;
                                    similarity = frame.similarity;
                                    edits = frame.edits;
                                    match_start = frame.match_start;
                                    call_stack = frame.call_stack;
                                }
                            }
                        } else {
                            state = *next;
                        }
                    } else {
                        state = *next;
                    }
                }

                State::CaptureStart { index, next } => {
                    // Start capturing
                    captures.start_capture(*index, pos);
                    state = *next;
                }

                State::CaptureEnd { index, next } => {
                    // End capturing
                    captures.end_capture(*index, pos);
                    state = *next;
                }

                State::Anchor { kind, next } => {
                    // Handle anchors
                    use crate::parser::ast::Anchor;
                    let matches = match kind {
                        Anchor::Start => pos == 0,
                        Anchor::End => pos == text.len(),
                        Anchor::WordBoundary => self.is_word_boundary_at(text_bytes, pos),
                        Anchor::NotWordBoundary => !self.is_word_boundary_at(text_bytes, pos),
                        Anchor::WordStart => self.is_word_start_at(text_bytes, pos),
                        Anchor::WordEnd => self.is_word_end_at(text_bytes, pos),
                    };

                    if matches {
                        state = *next;
                    } else {
                        let frame = backtrack_stack.pop()?;
                        state = frame.state;
                        pos = frame.pos;
                        captures = frame.captures;
                        similarity = frame.similarity;
                        edits = frame.edits;
                        match_start = frame.match_start;
                        call_stack = frame.call_stack;
                    }
                }

                State::Lookahead {
                    positive: _,
                    nfa: _,
                    literals: _,
                    next,
                } => {
                    // For lookahead, we need to evaluate the expression
                    // This is complex - skip for now
                    state = *next;
                }

                State::LookaheadLiteral {
                    positive,
                    literal,
                    next,
                } => {
                    // Inline literal lookahead - check if literal exists at current position
                    let matched = text.len() >= pos + literal.len()
                        && &text.as_bytes()[pos..pos + literal.len()] == literal.as_slice();
                    if matched == *positive {
                        state = *next;
                    } else {
                        return None;
                    }
                }

                State::Lookbehind {
                    positive: _,
                    nfa: _,
                    literals: _,
                    bridge: _,
                    next,
                } => {
                    // For lookbehind, similar complexity
                    state = *next;
                }

                State::LookbehindLiteral {
                    positive,
                    literal,
                    next,
                } => {
                    // Inline literal lookbehind - check if literal exists before current position
                    let matched = pos >= literal.len()
                        && &text.as_bytes()[pos - literal.len()..pos] == literal.as_slice();
                    if matched == *positive {
                        state = *next;
                    } else {
                        return None;
                    }
                }

                State::Backreference { group, next, .. } => {
                    // Try to match a backreference
                    if let Some(captured) = captures.get(*group) {
                        let captured_text = &text[captured.0..captured.1];
                        let remaining = &text[pos..];
                        if remaining.starts_with(captured_text) {
                            pos += captured_text.len();
                            state = *next;
                        } else {
                            let frame = backtrack_stack.pop()?;
                            state = frame.state;
                            pos = frame.pos;
                            captures = frame.captures;
                            similarity = frame.similarity;
                            edits = frame.edits;
                            match_start = frame.match_start;
                            call_stack = frame.call_stack;
                        }
                    } else {
                        state = *next;
                    }
                }

                State::Split { branches, greedy } => {
                    // Choice point - save state for backtracking
                    if *greedy {
                        // Greedy: try first branch, save others
                        state = branches[0];
                        for &b in &branches[1..] {
                            backtrack_stack.push(BacktrackFrame {
                                state: b,
                                pos,
                                captures: captures.clone(),
                                similarity,
                                edits: edits.clone(),
                                match_start,
                                recursion_depth: depth,
                                call_stack: call_stack.clone(),
                            });
                        }
                    } else {
                        // Non-greedy: try last branch first
                        state = branches[branches.len() - 1];
                        for i in (0..branches.len() - 1).rev() {
                            backtrack_stack.push(BacktrackFrame {
                                state: branches[i],
                                pos,
                                captures: captures.clone(),
                                similarity,
                                edits: edits.clone(),
                                match_start,
                                recursion_depth: depth,
                                call_stack: call_stack.clone(),
                            });
                        }
                    }
                }

                State::ResetMatchStart { next } => {
                    // \K - reset match start
                    match_start = pos;
                    state = *next;
                }

                State::AtomicGroup { nfa, next } => {
                    // Atomic group: match the sub-NFA without backtracking
                    // Build a sub-matcher and run it
                    let _sub_matcher =
                        BacktrackMatcher::new(nfa, self.fuzzy_bridge, 0, self.config.clone());
                    // Note: Atomic groups need to match the sub-NFA from the current position
                    // This is a simplified implementation
                    state = *next;
                }

                State::RecursivePattern { next } => {
                    // (?R) - call the whole pattern: start at the NFA start, and
                    // return when the accept state (0) is reached.
                    let target = Some((self.nfa.start, 0usize));
                    if let Some((start_state, end_state)) =
                        self.recursion_target(target, pos, &call_stack)
                    {
                        call_stack.push(CallFrame {
                            end_state,
                            return_state: *next,
                            start_pos: pos,
                        });
                        state = start_state;
                    } else {
                        let frame = backtrack_stack.pop()?;
                        state = frame.state;
                        pos = frame.pos;
                        captures = frame.captures;
                        similarity = frame.similarity;
                        edits = frame.edits;
                        match_start = frame.match_start;
                        depth = frame.recursion_depth;
                        call_stack = frame.call_stack;
                    }
                }

                State::RecursiveGroup { group, next } => {
                    // (?0) = whole pattern; (?1), (?2), … = a numbered group.
                    let target = if *group == 0 {
                        Some((self.nfa.start, 0usize))
                    } else {
                        self.nfa.group_states.get(*group - 1).copied()
                    };
                    if let Some((start_state, end_state)) =
                        self.recursion_target(target, pos, &call_stack)
                    {
                        call_stack.push(CallFrame {
                            end_state,
                            return_state: *next,
                            start_pos: pos,
                        });
                        state = start_state;
                    } else {
                        let frame = backtrack_stack.pop()?;
                        state = frame.state;
                        pos = frame.pos;
                        captures = frame.captures;
                        similarity = frame.similarity;
                        edits = frame.edits;
                        match_start = frame.match_start;
                        depth = frame.recursion_depth;
                        call_stack = frame.call_stack;
                    }
                }

                State::RecursiveNamedGroup { name, next } => {
                    // (?&name) / (?P>name) - call a named group.
                    let target = self.nfa.named_group_states.get(name).copied();
                    if let Some((start_state, end_state)) =
                        self.recursion_target(target, pos, &call_stack)
                    {
                        call_stack.push(CallFrame {
                            end_state,
                            return_state: *next,
                            start_pos: pos,
                        });
                        state = start_state;
                    } else {
                        let frame = backtrack_stack.pop()?;
                        state = frame.state;
                        pos = frame.pos;
                        captures = frame.captures;
                        similarity = frame.similarity;
                        edits = frame.edits;
                        match_start = frame.match_start;
                        depth = frame.recursion_depth;
                        call_stack = frame.call_stack;
                    }
                }

                State::Handler { name: _, next } => {
                    // (?call:name) - invoke custom handler
                    // For now, just continue to next state
                    state = *next;
                }
            }
        }
    }

    /// Resolve and guard a recursive subroutine call. Returns the
    /// `(start_state, end_state)` to enter, or `None` if the call is invalid
    /// (unknown group), exceeds the depth limit, or is left recursion — a call
    /// to the same subpattern at the same text position with no progress, which
    /// would otherwise loop forever.
    fn recursion_target(
        &self,
        target: Option<(StateId, StateId)>,
        pos: usize,
        call_stack: &[CallFrame],
    ) -> Option<(StateId, StateId)> {
        let (start_state, end_state) = target?;
        if call_stack.len() >= self.max_recursion {
            return None;
        }
        if call_stack
            .iter()
            .any(|f| f.end_state == end_state && f.start_pos == pos)
        {
            return None;
        }
        Some((start_state, end_state))
    }

    /// Check if position is at a word boundary.
    #[allow(clippy::unused_self)]
    fn word_before(text: &[u8], pos: usize) -> bool {
        if pos == 0 {
            return false;
        }
        let mut start = pos - 1;
        while start > 0 && (text[start] & 0xC0) == 0x80 {
            start -= 1;
        }
        let ch = &text[start..pos];
        !ch.is_empty() && ch.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_')
    }

    fn word_after(text: &[u8], pos: usize) -> bool {
        if pos >= text.len() {
            return false;
        }
        let mut end = pos;
        while end < text.len() && (text[end] & 0xC0) == 0x80 {
            end += 1;
        }
        let ch = &text[pos..end.min(text.len())];
        !ch.is_empty() && ch.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_')
    }

    #[allow(clippy::unused_self)]
    fn is_word_boundary_at(&self, text: &[u8], pos: usize) -> bool {
        Self::word_before(text, pos) != Self::word_after(text, pos)
    }

    /// `\m` — start of a word.
    #[allow(clippy::unused_self)]
    fn is_word_start_at(&self, text: &[u8], pos: usize) -> bool {
        !Self::word_before(text, pos) && Self::word_after(text, pos)
    }

    /// `\M` — end of a word.
    #[allow(clippy::unused_self)]
    fn is_word_end_at(&self, text: &[u8], pos: usize) -> bool {
        Self::word_before(text, pos) && !Self::word_after(text, pos)
    }
}
