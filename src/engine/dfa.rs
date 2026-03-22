//! DFA (Deterministic Finite Automaton) for fast exact matching.
//!
//! This module implements lazy DFA construction from NFA using subset construction.
//! For patterns without fuzzy matching, DFA provides O(1) per character matching.

#![allow(
    clippy::needless_range_loop,
    clippy::match_same_arms,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::items_after_statements,
    clippy::inline_always,
    clippy::float_cmp,
    clippy::allow_attributes,
    let_underscore_drop
)]

// Note: iter_over_flow_is_subslice is not a valid lint

use std::collections::HashMap;

use memchr::{memchr, memrchr, memmem};

use super::fuzzy_bridge::FuzzyBridge;
use super::hash::FxHashMap;
use super::simd_class::{AsciiClassBitmap, RevSearchRanges, TeddySearch};
use crate::ir::{Nfa, State, StateId};
use crate::parser::Anchor;

/// An extended NFA state that can track position within a `FuzzyLiteral`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ExtendedState {
    /// The NFA state ID.
    state_id: StateId,
    /// Offset within a `FuzzyLiteral` pattern (0 means at start of pattern).
    /// None for non-FuzzyLiteral states.
    literal_offset: Option<usize>,
}

impl ExtendedState {
    fn new(state_id: StateId) -> Self {
        ExtendedState {
            state_id,
            literal_offset: None,
        }
    }

    fn with_offset(state_id: StateId, offset: usize) -> Self {
        ExtendedState {
            state_id,
            literal_offset: Some(offset),
        }
    }
}

/// A set of NFA states, represented as a sorted vector for hashing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NfaStateSet(Vec<ExtendedState>);

impl NfaStateSet {
    fn new() -> Self {
        NfaStateSet(Vec::new())
    }

    fn with_capacity(cap: usize) -> Self {
        NfaStateSet(Vec::with_capacity(cap))
    }

    fn insert(&mut self, state: ExtendedState) {
        if let Err(pos) = self.0.binary_search(&state) {
            self.0.insert(pos, state);
        }
    }

    fn contains(&self, state: &ExtendedState) -> bool {
        self.0.binary_search(state).is_ok()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &ExtendedState> {
        self.0.iter()
    }
}

/// DFA state ID.
type DfaStateId = u32;

/// Sentinel value meaning "transition not yet computed".
const TRANS_UNKNOWN: u32 = u32::MAX;
/// Sentinel value meaning "dead state (no valid transition)".
const TRANS_DEAD: u32 = u32::MAX - 1;

/// Dense transition table for ASCII characters (0-127).
/// Uses u32 state IDs with sentinel values for unknown/dead transitions.
#[derive(Clone)]
struct AsciiTransitions {
    /// Transitions for ASCII bytes 0-127.
    /// `TRANS_UNKNOWN` = not computed, `TRANS_DEAD` = dead state.
    table: Box<[u32; 128]>,
}

impl AsciiTransitions {
    fn new() -> Self {
        AsciiTransitions {
            table: Box::new([TRANS_UNKNOWN; 128]),
        }
    }

    #[inline]
    fn get(&self, byte: u8) -> u32 {
        self.table[byte as usize]
    }

    #[inline]
    fn set(&mut self, byte: u8, state: u32) {
        self.table[byte as usize] = state;
    }
}

impl std::fmt::Debug for AsciiTransitions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Only show non-UNKNOWN transitions for brevity
        let computed_count = self.table.iter().filter(|&&v| v != TRANS_UNKNOWN).count();
        f.debug_struct("AsciiTransitions")
            .field("computed_count", &computed_count)
            .finish()
    }
}

/// A DFA state with transitions.
#[derive(Debug, Clone)]
struct DfaState {
    /// The set of NFA states this DFA state represents.
    nfa_states: NfaStateSet,
    /// Whether this state is accepting.
    is_accept: bool,
    /// Whether this state contains a start anchor (^).
    has_start_anchor: bool,
    /// Whether this state contains an end anchor ($).
    has_end_anchor: bool,
    /// Dense transition table for ASCII characters (fast path).
    ascii_transitions: AsciiTransitions,
    /// Sparse transitions for non-ASCII characters.
    unicode_transitions: FxHashMap<char, u32>,
}

/// Prefilter for fast candidate position detection.
#[derive(Debug, Clone)]
enum DfaPrefilter {
    /// No prefiltering - scan every position.
    None,
    /// Single byte prefilter using memchr.
    SingleByte(u8),
    /// Two bytes using memchr2.
    TwoBytes(u8, u8),
    /// Three bytes using memchr3 (for alternations).
    ThreeBytes(u8, u8, u8),
    /// More than 3 bytes - fallback to byte set scanning.
    ManyBytes(Vec<u8>),
    /// Literal prefix using memmem.
    Literal(Vec<u8>),
    /// Character class ranges for SIMD-accelerated range search (e.g., [0-9]).
    CharClassRanges(Vec<(u8, u8)>),
}

/// Lazy DFA for fast exact matching.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct Dfa {
    /// The source NFA.
    nfa: Nfa,
    /// Literal patterns for `FuzzyLiteral` expansion.
    literal_texts: Vec<String>,
    /// DFA states.
    states: Vec<DfaState>,
    /// Map from NFA state set to DFA state ID.
    state_cache: HashMap<NfaStateSet, DfaStateId>,
    /// The start state ID.
    start: DfaStateId,
    /// Whether the pattern is anchored at the start (^).
    anchored_start: bool,
    /// Whether the pattern is anchored at the end ($).
    anchored_end: bool,
    /// Whether matching is case-insensitive.
    case_insensitive: bool,
    /// Whether multiline mode is enabled (^ and $ match at line boundaries).
    multi_line: bool,
    /// Pre-compiled character class bitmaps indexed by NFA state ID.
    /// Enables O(1) ASCII character class membership testing.
    char_class_bitmaps: Vec<Option<AsciiClassBitmap>>,
    /// Prefilter for fast candidate position detection.
    prefilter: DfaPrefilter,
    /// Exact mode: if true, skip fuzzy literal handling (optimization for exact patterns)
    exact_mode: bool,
    /// Fast path: character class plus pattern (e.g., \d+, \w+, \s+)
    #[allow(dead_code)]
    fast_path_char_class_plus: bool,
    /// Fast path: type of character class for `char_class_plus` pattern
    #[allow(dead_code)]
    fast_path_char_class_type: Option<&'static str>,
    /// Fast path: literal for simple literal matching
    #[allow(dead_code)]
    fast_path_literal: Option<String>,
}

/// Result of a DFA match.
#[derive(Debug, Clone)]
pub struct DfaMatch {
    /// Start position (byte index).
    pub start: usize,
    /// End position (byte index).
    pub end: usize,
}

impl Dfa {
    /// Create a new DFA from an NFA.
    /// Returns None if the NFA contains states that can't be converted to DFA
    /// (fuzzy matching with edits, lookahead/lookbehind, backreferences).
    ///
    /// If `bridge` is provided, exact `FuzzyLiteral` states will be expanded.
    /// If `case_insensitive` is true, matching will be case-insensitive.
    /// If `multi_line` is true, ^ and $ will match at line boundaries.
    /// If `similarity_threshold` >= 1.0, fast paths can be used (exact matching).
    #[must_use]
    pub fn from_nfa(
        nfa: &Nfa,
        bridge: Option<&FuzzyBridge>,
        case_insensitive: bool,
        multi_line: bool,
        _similarity_threshold: f32,
    ) -> Option<Self> {
        // Check if NFA is DFA-compatible
        if !Self::is_dfa_compatible(nfa, bridge) {
            return None;
        }

        // Extract literal texts from bridge
        let literal_texts = if let Some(b) = bridge {
            (0..b.pattern_count())
                .filter_map(|i| b.pattern_text(i).map(ToString::to_string))
                .collect()
        } else {
            Vec::new()
        };

        // Pre-compile character class bitmaps for fast matching
        let char_class_bitmaps: Vec<Option<AsciiClassBitmap>> = nfa
            .states
            .iter()
            .map(|state| {
                if let State::Char { class, .. } = state {
                    Some(AsciiClassBitmap::from_hir_class(class))
                } else {
                    None
                }
            })
            .collect();

        // Extract literal prefix for prefiltering
        let prefilter = Self::extract_prefilter(nfa, &literal_texts, case_insensitive);

        // Determine if we're in exact mode: no FuzzyLiteral states in the NFA at all
        // When true, we skip fuzzy literal handling entirely for performance
        let exact_mode = !nfa
            .states
            .iter()
            .any(|s| matches!(s, State::FuzzyLiteral { .. }));

        // Only use fast paths when default_edits == 0 (exact matching)
        // For fuzzy matching, we need the full DFA to compute similarity
        let use_fast_paths = exact_mode;
        // Detect fast path patterns - only for named character classes (\d, \w, \s)
        // Only enable when there's NO fuzzy bridge at all AND exact matching
        let fast_path_char_class_type = if use_fast_paths {
            nfa.get_char_class_type()
        } else {
            None
        };
        let fast_path_char_class_plus = use_fast_paths
            && bridge.is_none()
            && fast_path_char_class_type.is_some()
            && nfa.is_char_class_plus()
            && nfa.states.len() <= 10;

        // Detect simple literal patterns for fast path
        // Only enable when there's NO fuzzy bridge at all (bridge.is_none())
        // When bridge is Some, even if all literals are exact, we still want to use the full DFA
        let fast_path_literal =
            if use_fast_paths && bridge.is_none() && exact_mode && !fast_path_char_class_plus {
                Self::detect_simple_literal(nfa)
            } else {
                None
            };

        let mut dfa = Dfa {
            nfa: nfa.clone(),
            literal_texts,
            states: Vec::new(),
            state_cache: HashMap::new(),
            start: 0,
            anchored_start: false,
            anchored_end: false,
            case_insensitive,
            multi_line,
            char_class_bitmaps,
            prefilter,
            exact_mode,
            fast_path_char_class_plus,
            fast_path_char_class_type,
            fast_path_literal,
        };

        // Compute epsilon closure of start state
        let mut start_set = NfaStateSet::new();
        dfa.epsilon_closure(nfa.start, &mut start_set);

        // Check for start anchor
        dfa.anchored_start = start_set.iter().any(|s| {
            matches!(
                &nfa.states[s.state_id],
                State::Anchor {
                    kind: Anchor::Start,
                    ..
                }
            )
        });

        // Check for end anchor - scan all NFA states
        dfa.anchored_end = nfa.states.iter().any(|s| {
            matches!(
                s,
                State::Anchor {
                    kind: Anchor::End,
                    ..
                }
            )
        });

        // Create start state
        let start_id = dfa.get_or_create_state(start_set);
        dfa.start = start_id;

        Some(dfa)
    }

    /// Extract a literal prefix from the NFA for prefiltering.
    fn extract_prefilter(
        nfa: &Nfa,
        literal_texts: &[String],
        case_insensitive: bool,
    ) -> DfaPrefilter {
        // For patterns where the first element is a broad character class (like [\w.+-]+)
        // followed by a literal, the prefilter becomes ineffective because:
        // 1. Character class first bytes cover almost everything
        // 2. Even if we use the literal as prefilter (e.g., @), find_at() fails
        //    because match starts BEFORE the literal
        // In these cases, skip the prefilter and use linear scan
        if !literal_texts.is_empty() && literal_texts[0].len() <= 2 {
            // Check if NFA starts with a character class that has many possible first bytes
            let starts_with_broad_class = Self::nfa_starts_with_broad_char_class(nfa);
            if starts_with_broad_class {
                // Skip prefilter - it would be ineffective
                return DfaPrefilter::None;
            }
        }

        let mut prefix = Vec::new();
        let mut visited = vec![false; nfa.states.len()];
        let mut current = nfa.start;

        // Follow the NFA from start, collecting literal bytes
        loop {
            if visited[current] {
                break;
            }
            visited[current] = true;

            match &nfa.states[current] {
                State::Epsilon { targets } if targets.len() == 1 => {
                    current = targets[0];
                }
                State::Anchor { next, .. }
                | State::CaptureStart { next, .. }
                | State::CaptureEnd { next, .. } => {
                    current = *next;
                }
                State::Char { class, .. } => {
                    // Check if this is a single character class (only single chars, not ranges or named classes)
                    // Note: We don't extract prefilters for ranges or named classes because they can match
                    // multiple different first bytes, making prefilters less effective.
                    if class.chars.len() == 1
                        && class.ranges.is_empty()
                        && class.named.is_empty()
                        && !class.negated
                    {
                        let ch = class.chars[0];
                        if ch.is_ascii() {
                            prefix.push(ch as u8);
                            // Continue to next state would require computing epsilon closure
                            // which is complex, so we stop after first char for now
                            break;
                        }
                    }

                    // For character classes with small ranges or named classes,
                    // collect all possible first bytes and use ManyBytes prefilter
                    // Only do this for ASCII-safe classes to avoid complexity
                    if !class.negated {
                        let mut first_bytes = Vec::new();

                        // Add single chars
                        for &ch in &class.chars {
                            if ch.is_ascii() {
                                first_bytes.push(ch as u8);
                            }
                        }

                        // Add single characters (ASCII and non-ASCII)
                        for &ch in &class.chars {
                            if ch.is_ascii() {
                                let b = ch as u8;
                                if !first_bytes.contains(&b) {
                                    first_bytes.push(b);
                                }
                            } else {
                                // For non-ASCII chars, add UTF-8 leading byte
                                let mut buf = [0u8; 4];
                                let encoded = ch.encode_utf8(&mut buf);
                                let first_byte = encoded.as_bytes()[0];
                                if !first_bytes.contains(&first_byte) {
                                    first_bytes.push(first_byte);
                                }
                            }
                        }

                        // Add ranges (ASCII and non-ASCII)
                        for &(start, end) in &class.ranges {
                            if start.is_ascii() && end.is_ascii() {
                                for b in (start as u8)..=(end as u8) {
                                    if !first_bytes.contains(&b) {
                                        first_bytes.push(b);
                                    }
                                }
                            } else {
                                // For non-ASCII ranges, add UTF-8 leading bytes
                                Self::add_utf8_leading_bytes(start, end, &mut first_bytes);
                            }
                        }

                        // Add named classes (common ASCII ones)
                        for named in &class.named {
                            match named {
                                crate::parser::NamedClass::Digit => {
                                    // Add '0'-'9' (ASCII only for prefilter efficiency)
                                    for b in b'0'..=b'9' {
                                        if !first_bytes.contains(&b) {
                                            first_bytes.push(b);
                                        }
                                    }
                                }
                                crate::parser::NamedClass::Word => {
                                    // Add 'a'-'z', 'A'-'Z', '0'-'9', '_'
                                    for b in b'a'..=b'z' {
                                        if !first_bytes.contains(&b) {
                                            first_bytes.push(b);
                                        }
                                    }
                                    for b in b'A'..=b'Z' {
                                        if !first_bytes.contains(&b) {
                                            first_bytes.push(b);
                                        }
                                    }
                                    for b in b'0'..=b'9' {
                                        if !first_bytes.contains(&b) {
                                            first_bytes.push(b);
                                        }
                                    }
                                    if !first_bytes.contains(&b'_') {
                                        first_bytes.push(b'_');
                                    }
                                    // Add common Unicode word char first bytes
                                    Self::add_utf8_leading_bytes(
                                        '\u{00C0}',
                                        '\u{024F}',
                                        &mut first_bytes,
                                    );
                                    Self::add_utf8_leading_bytes(
                                        '\u{0400}',
                                        '\u{04FF}',
                                        &mut first_bytes,
                                    );
                                    Self::add_utf8_leading_bytes(
                                        '\u{0900}',
                                        '\u{097F}',
                                        &mut first_bytes,
                                    );
                                }
                                crate::parser::NamedClass::Whitespace => {
                                    // Add ASCII whitespace
                                    for &b in b" \t\n\r" {
                                        if !first_bytes.contains(&b) {
                                            first_bytes.push(b);
                                        }
                                    }
                                    // Add common Unicode whitespace first bytes
                                    Self::add_utf8_leading_bytes(
                                        '\u{0085}',
                                        '\u{0085}',
                                        &mut first_bytes,
                                    );
                                    Self::add_utf8_leading_bytes(
                                        '\u{00A0}',
                                        '\u{00A0}',
                                        &mut first_bytes,
                                    );
                                    Self::add_utf8_leading_bytes(
                                        '\u{2000}',
                                        '\u{200A}',
                                        &mut first_bytes,
                                    );
                                }
                                // Skip other named classes
                                _ => {}
                            }
                        }

                        // Only use if we have a small number of bytes (to keep prefilter efficient)
                        if !first_bytes.is_empty() && first_bytes.len() <= 10 {
                            return Self::make_multi_byte_prefilter(&first_bytes, case_insensitive);
                        }
                    }

                    break;
                }
                State::FuzzyLiteral { pattern_index, .. } => {
                    // Get the literal text
                    if let Some(text) = literal_texts.get(*pattern_index) {
                        prefix.extend(text.as_bytes().iter().take(8)); // Cap at 8 bytes
                    }
                    break;
                }
                State::Split { branches, .. } => {
                    // For alternations, collect first bytes from each branch
                    let first_bytes =
                        Self::collect_first_bytes_from_branches(nfa, branches, literal_texts);
                    if !first_bytes.is_empty() {
                        return Self::make_multi_byte_prefilter(&first_bytes, case_insensitive);
                    }
                    break;
                }
                _ => break,
            }
        }

        if prefix.is_empty() {
            return DfaPrefilter::None;
        }

        // For case-insensitive, we can only use single-byte or two-byte prefilters
        // because memmem is case-sensitive
        if case_insensitive {
            let b = prefix[0];
            if b.is_ascii_alphabetic() {
                return DfaPrefilter::TwoBytes(b.to_ascii_lowercase(), b.to_ascii_uppercase());
            }
            // Non-alphabetic first byte - can use single byte
            return DfaPrefilter::SingleByte(b);
        }

        // For case-sensitive single byte, use SingleByte
        if prefix.len() == 1 {
            return DfaPrefilter::SingleByte(prefix[0]);
        }

        // For multi-byte prefix, use Literal
        DfaPrefilter::Literal(prefix)
    }

    /// Detect if NFA is a simple literal pattern (no special constructs).
    /// Returns the literal string if detected, None otherwise.
    fn detect_simple_literal(nfa: &Nfa) -> Option<String> {
        // Must be simple NFA (few states)
        if nfa.states.len() > 20 {
            return None;
        }

        // Must have only Char and Accept states (no splits, no anchors, etc.)
        let mut literal = String::new();
        let mut current = nfa.start;
        let mut visited = vec![false; nfa.states.len()];

        while current < nfa.states.len() && !visited[current] {
            visited[current] = true;

            match &nfa.states[current] {
                State::Accept => {
                    // Reached accept state - return the literal
                    return Some(literal);
                }
                State::Char { class, next } => {
                    // Must have NO named character classes (like \d, \w, \s)
                    // Must have exactly one char in the class (not a range)
                    if !class.named.is_empty() || class.ranges.len() != 1 || class.chars.len() != 1
                    {
                        return None;
                    }
                    let (start, end) = class.ranges[0];
                    if start != end || !start.is_ascii() {
                        return None;
                    }
                    literal.push(start);
                    current = *next;
                }
                State::Epsilon { targets } if targets.len() == 1 => {
                    current = targets[0];
                }
                _ => {
                    return None;
                }
            }
        }

        None
    }

    /// Check if NFA starts with a character class that has many possible first bytes.
    /// Returns true if the first character-accepting state is a broad character class
    /// (more than ~10 possible first bytes), making prefilters ineffective.
    fn nfa_starts_with_broad_char_class(nfa: &Nfa) -> bool {
        let mut visited = vec![false; nfa.states.len()];
        let mut current = nfa.start;

        loop {
            if visited[current] {
                return false;
            }
            visited[current] = true;

            match &nfa.states[current] {
                State::Epsilon { targets } if targets.len() == 1 => {
                    current = targets[0];
                }
                State::Anchor { next, .. }
                | State::CaptureStart { next, .. }
                | State::CaptureEnd { next, .. }
                | State::ResetMatchStart { next, .. } => {
                    current = *next;
                }
                State::Char { class, .. } => {
                    // Count possible first bytes
                    let mut count = 0;
                    count += class.chars.len();
                    for &(start, end) in &class.ranges {
                        count += (end as usize).saturating_sub(start as usize) + 1;
                    }
                    // Add named classes (estimate)
                    for _ in &class.named {
                        count += 10; // Word = a-zA-Z0-9_, Digit = 0-9, etc.
                    }

                    // If more than ~10 possible first bytes, consider it "broad"
                    return count > 10;
                }
                State::Split { branches, .. } => {
                    // Check if any branch starts with a broad class
                    return branches.iter().any(|&b| {
                        Self::nfa_starts_with_broad_char_class_inner(
                            nfa,
                            b,
                            &mut vec![false; nfa.states.len()],
                        )
                    });
                }
                _ => {
                    return false;
                }
            }
        }
    }

    fn nfa_starts_with_broad_char_class_inner(
        nfa: &Nfa,
        state_id: StateId,
        visited: &mut [bool],
    ) -> bool {
        if state_id >= visited.len() || visited[state_id] {
            return false;
        }
        visited[state_id] = true;

        match &nfa.states[state_id] {
            State::Epsilon { targets } => targets
                .iter()
                .any(|&t| Self::nfa_starts_with_broad_char_class_inner(nfa, t, visited)),
            State::Char { class, .. } => {
                let mut count = 0;
                count += class.chars.len();
                for &(start, end) in &class.ranges {
                    count += (end as usize).saturating_sub(start as usize) + 1;
                }
                for _ in &class.named {
                    count += 10;
                }
                count > 10
            }
            _ => false,
        }
    }

    /// Collect first bytes from all branches of a Split state.
    fn collect_first_bytes_from_branches(
        nfa: &Nfa,
        branches: &[StateId],
        literal_texts: &[String],
    ) -> Vec<u8> {
        let mut first_bytes = Vec::new();

        for &branch in branches {
            if let Some(byte) = Self::get_first_byte(nfa, branch, literal_texts)
                && !first_bytes.contains(&byte)
            {
                first_bytes.push(byte);
            }
        }

        first_bytes
    }

    /// Get the first byte of a pattern starting at a given NFA state.
    fn get_first_byte(nfa: &Nfa, start: StateId, literal_texts: &[String]) -> Option<u8> {
        let mut visited = vec![false; nfa.states.len()];
        let mut current = start;

        loop {
            if current >= nfa.states.len() || visited[current] {
                return None;
            }
            visited[current] = true;

            match &nfa.states[current] {
                State::Epsilon { targets } if !targets.is_empty() => {
                    current = targets[0];
                }
                State::CaptureStart { next, .. } | State::CaptureEnd { next, .. } => {
                    current = *next;
                }
                State::Char { class, .. } => {
                    // Get first byte from character class
                    if !class.chars.is_empty() {
                        let ch = class.chars[0];
                        // Return the first UTF-8 byte for any character (ASCII or non-ASCII)
                        let mut buf = [0u8; 4];
                        let encoded = ch.encode_utf8(&mut buf);
                        return Some(encoded.as_bytes()[0]);
                    }
                    if !class.ranges.is_empty() {
                        let (start, _) = class.ranges[0];
                        if start.is_ascii() {
                            return Some(start as u8);
                        }
                        // For non-ASCII ranges, return the first byte of the range start
                        let mut buf = [0u8; 4];
                        let encoded = start.encode_utf8(&mut buf);
                        return Some(encoded.as_bytes()[0]);
                    }
                    return None;
                }
                State::FuzzyLiteral { pattern_index, .. } => {
                    if let Some(text) = literal_texts.get(*pattern_index) {
                        return text.as_bytes().first().copied();
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }

    /// Add UTF-8 leading bytes for a Unicode range to the prefilter.
    /// This handles 2-byte, 3-byte, and 4-byte UTF-8 encodings.
    fn add_utf8_leading_bytes(start: char, end: char, bytes: &mut Vec<u8>) {
        let start_u32 = start as u32;
        let end_u32 = end as u32;
        let step = (end_u32 - start_u32).clamp(1, 16);
        let mut code = start_u32;
        while code <= end_u32 {
            let ch = char::from_u32(code).unwrap_or(start);
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            let first_byte = encoded.as_bytes()[0];
            if !bytes.contains(&first_byte) {
                bytes.push(first_byte);
            }
            code = code.saturating_add(step);
            if code > end_u32 && code > start_u32 + step {
                break;
            }
        }
    }

    /// Convert a sorted list of bytes to contiguous ranges.
    /// For example: [0, 1, 2, 5, 6, 7, 10] -> [(0, 2), (5, 7), (10, 10)]
    fn bytes_to_ranges(bytes: &[u8]) -> Vec<(u8, u8)> {
        if bytes.is_empty() {
            return Vec::new();
        }

        let mut ranges = Vec::new();
        let mut start = bytes[0];
        let mut end = bytes[0];

        for &b in &bytes[1..] {
            if b == end + 1 {
                // Contiguous - extend the range
                end = b;
            } else {
                // Not contiguous - save current range and start new one
                ranges.push((start, end));
                start = b;
                end = b;
            }
        }
        ranges.push((start, end));
        ranges
    }

    /// Create a prefilter for multiple first bytes.
    fn make_multi_byte_prefilter(bytes: &[u8], case_insensitive: bool) -> DfaPrefilter {
        if bytes.is_empty() {
            return DfaPrefilter::None;
        }

        // Expand bytes for case-insensitive matching
        let expanded: Vec<u8> = if case_insensitive {
            let mut result = Vec::new();
            for &b in bytes {
                if b.is_ascii_alphabetic() {
                    let lower = b.to_ascii_lowercase();
                    let upper = b.to_ascii_uppercase();
                    if !result.contains(&lower) {
                        result.push(lower);
                    }
                    if !result.contains(&upper) {
                        result.push(upper);
                    }
                } else if !result.contains(&b) {
                    result.push(b);
                }
            }
            result
        } else {
            bytes.to_vec()
        };

        match expanded.len() {
            0 => DfaPrefilter::None,
            1 => DfaPrefilter::SingleByte(expanded[0]),
            2 => DfaPrefilter::TwoBytes(expanded[0], expanded[1]),
            3 => DfaPrefilter::ThreeBytes(expanded[0], expanded[1], expanded[2]),
            _ => {
                // More than 3 bytes - check if we can use SIMD-accelerated range search
                // Sort bytes and convert to contiguous ranges
                let mut sorted = expanded.clone();
                sorted.sort();
                sorted.dedup();
                
                let ranges = Self::bytes_to_ranges(&sorted);
                
                // Use CharClassRanges if we have at most 3 ranges (SIMD limit)
                // Otherwise fall back to ManyBytes
                if ranges.len() <= 3 {
                    DfaPrefilter::CharClassRanges(ranges)
                } else {
                    DfaPrefilter::ManyBytes(sorted)
                }
            }
        }
    }

    /// Check if an NFA can be converted to DFA.
    fn is_dfa_compatible(nfa: &Nfa, bridge: Option<&FuzzyBridge>) -> bool {
        for state in &nfa.states {
            match state {
                State::Accept
                | State::Epsilon { .. }
                | State::ResetMatchStart { .. }
                | State::Char { .. }
                | State::Split { .. }
                | State::CaptureStart { .. }
                | State::CaptureEnd { .. } => {}

                // Only Start and End anchors are supported by DFA
                // Word boundaries require NFA matching
                State::Anchor { kind, .. } => {
                    use crate::parser::ast::Anchor;
                    match kind {
                        Anchor::Start | Anchor::End => {}
                        Anchor::WordBoundary | Anchor::NotWordBoundary => return false,
                    }
                }

                // FuzzyLiteral is OK if it's exact (no edits)
                State::FuzzyLiteral {
                    limits,
                    pattern_index,
                    ..
                } => {
                    let max_edits = limits.as_ref().map(|l| {
                        l.get_edits().unwrap_or_else(|| {
                            let i = l.get_insertions().unwrap_or(0);
                            let d = l.get_deletions().unwrap_or(0);
                            let s = l.get_substitutions().unwrap_or(0);
                            let t = l.get_swaps().unwrap_or(0);
                            i.saturating_add(d).saturating_add(s).saturating_add(t)
                        })
                    });
                    // Only compatible if exact and we have the pattern text
                    if (max_edits.is_none() || max_edits == Some(0))
                        && bridge.is_some_and(|b| b.pattern_text(*pattern_index).is_some())
                    {
                        continue;
                    }
                    return false;
                }

                // These can't be converted to DFA
                State::FuzzyChar { .. }
                | State::Lookahead { .. }
                | State::Lookbehind { .. }
                | State::Backreference { .. }
                | State::AtomicGroup { .. }
                | State::RecursivePattern { .. }
                | State::RecursiveGroup { .. }
                | State::RecursiveNamedGroup { .. }
                | State::Handler { .. } => return false,
            }
        }
        true
    }

    /// Compute the epsilon closure of an NFA state.
    fn epsilon_closure(&self, state: StateId, result: &mut NfaStateSet) {
        self.epsilon_closure_ext(ExtendedState::new(state), result);
    }

    /// Compute the epsilon closure of an extended NFA state.
    fn epsilon_closure_ext(&self, ext_state: ExtendedState, result: &mut NfaStateSet) {
        if result.contains(&ext_state) {
            return;
        }

        let state = ext_state.state_id;

        match &self.nfa.states[state] {
            State::Epsilon { targets } => {
                result.insert(ext_state);
                for &target in targets {
                    self.epsilon_closure_ext(ExtendedState::new(target), result);
                }
            }
            State::Split { branches, .. } => {
                result.insert(ext_state);
                for &branch in branches {
                    self.epsilon_closure_ext(ExtendedState::new(branch), result);
                }
            }
            State::CaptureStart { next, .. } | State::CaptureEnd { next, .. } => {
                // Skip capture markers (they don't consume input)
                result.insert(ext_state);
                self.epsilon_closure_ext(ExtendedState::new(*next), result);
            }
            State::Anchor { next, .. } => {
                result.insert(ext_state);
                // Anchors don't consume input - follow to next state
                // We keep the anchor in the set so we can check it at match time
                self.epsilon_closure_ext(ExtendedState::new(*next), result);
            }
            State::FuzzyLiteral { .. } => {
                // For FuzzyLiteral, we start at offset 0
                if ext_state.literal_offset.is_none() {
                    // Initial entry - start at offset 0
                    result.insert(ExtendedState::with_offset(state, 0));
                } else {
                    // Already have an offset, keep it
                    result.insert(ext_state);
                }
            }
            _ => {
                result.insert(ext_state);
            }
        }
    }

    /// Get or create a DFA state for the given NFA state set.
    fn get_or_create_state(&mut self, nfa_states: NfaStateSet) -> DfaStateId {
        if let Some(&id) = self.state_cache.get(&nfa_states) {
            return id;
        }

        let is_accept = nfa_states
            .iter()
            .any(|ext| matches!(&self.nfa.states[ext.state_id], State::Accept));

        let has_start_anchor = nfa_states.iter().any(|ext| {
            matches!(
                &self.nfa.states[ext.state_id],
                State::Anchor {
                    kind: Anchor::Start,
                    ..
                }
            )
        });

        let has_end_anchor = nfa_states.iter().any(|ext| {
            matches!(
                &self.nfa.states[ext.state_id],
                State::Anchor {
                    kind: Anchor::End,
                    ..
                }
            )
        });

        let id = self.states.len() as DfaStateId;
        self.states.push(DfaState {
            nfa_states: nfa_states.clone(),
            is_accept,
            has_start_anchor,
            has_end_anchor,
            ascii_transitions: AsciiTransitions::new(),
            unicode_transitions: FxHashMap::default(),
        });
        self.state_cache.insert(nfa_states, id);
        id
    }

    /// Check if two characters match, considering case-insensitivity.
    /// Uses const generic to eliminate branch at compile time.
    #[inline(always)]
    fn chars_match<const CASE_INSENSITIVE: bool>(text_char: char, pattern_char: char) -> bool {
        if CASE_INSENSITIVE {
            // Use Unicode case folding for proper case-insensitive matching
            // For ASCII, use the fast path; for non-ASCII, use to_lowercase()
            if text_char.is_ascii() && pattern_char.is_ascii() {
                text_char.eq_ignore_ascii_case(&pattern_char)
            } else {
                // Unicode case folding: compare lowercase forms
                text_char.to_lowercase().eq(pattern_char.to_lowercase())
            }
        } else {
            text_char == pattern_char
        }
    }

    /// Compute the next DFA state for a given character.
    /// Dispatches to const generic implementation for branch elimination.
    #[inline(always)]
    fn next_state(&mut self, state_id: DfaStateId, ch: char) -> Option<DfaStateId> {
        if self.case_insensitive {
            if self.exact_mode {
                self.next_state_impl::<true, true>(state_id, ch)
            } else {
                self.next_state_impl::<true, false>(state_id, ch)
            }
        } else if self.exact_mode {
            self.next_state_impl::<false, true>(state_id, ch)
        } else {
            self.next_state_impl::<false, false>(state_id, ch)
        }
    }

    /// Const generic implementation of `next_state`.
    /// Uses dense ASCII table for fast O(1) lookups on ASCII characters.
    /// `CASE_INSENSITIVE`: whether to match case-insensitively
    /// `EXACT_MODE`: if true, skip fuzzy literal handling (for pure exact patterns)
    #[inline(always)]
    fn next_state_impl<const CASE_INSENSITIVE: bool, const EXACT_MODE: bool>(
        &mut self,
        state_id: DfaStateId,
        ch: char,
    ) -> Option<DfaStateId> {
        let state_idx = state_id as usize;

        // Fast path: ASCII character with dense table lookup
        if ch.is_ascii() {
            let byte = ch as u8;
            let cached = self.states[state_idx].ascii_transitions.get(byte);
            if cached != TRANS_UNKNOWN {
                return if cached == TRANS_DEAD {
                    None
                } else {
                    Some(cached)
                };
            }
        } else {
            // Non-ASCII: check sparse table
            if let Some(&cached) = self.states[state_idx].unicode_transitions.get(&ch) {
                return if cached == TRANS_DEAD {
                    None
                } else {
                    Some(cached)
                };
            }
        }

        // Cache miss - compute the transition
        let current = &self.states[state_idx];
        let mut next_set = NfaStateSet::with_capacity(current.nfa_states.0.len());

        // Collect states to process (to avoid borrowing issues)
        let nfa_states: Vec<_> = current.nfa_states.0.clone();

        for ext_state in nfa_states {
            let nfa_state = ext_state.state_id;
            match &self.nfa.states[nfa_state] {
                State::Char { class, next } => {
                    // Use pre-compiled bitmap for fast ASCII matching only.
                    // For non-ASCII, use the original class matcher which handles
                    // full Unicode character comparison correctly.
                    let matches = if ch.is_ascii() {
                        if nfa_state < self.char_class_bitmaps.len() {
                            if let Some(bitmap) = &self.char_class_bitmaps[nfa_state] {
                                let result = bitmap.contains(ch as u8);
                                if CASE_INSENSITIVE {
                                    result
                                        || bitmap.contains((ch as u8).to_ascii_lowercase())
                                        || bitmap.contains((ch as u8).to_ascii_uppercase())
                                } else {
                                    result
                                }
                            } else {
                                class.matches(ch)
                            }
                        } else {
                            class.matches(ch)
                        }
                    } else {
                        // Non-ASCII: use original class matcher
                        if CASE_INSENSITIVE {
                            class.matches(ch)
                                || class.matches(ch.to_ascii_lowercase())
                                || class.matches(ch.to_ascii_uppercase())
                        } else {
                            class.matches(ch)
                        }
                    };
                    if matches {
                        self.epsilon_closure(*next, &mut next_set);
                    }
                }
                State::FuzzyLiteral {
                    pattern_index,
                    next,
                    ..
                } => {
                    // Only handle fuzzy literals in non-exact mode
                    if !EXACT_MODE {
                        let offset = ext_state.literal_offset.unwrap_or(0);
                        if let Some(pattern) = self.literal_texts.get(*pattern_index) {
                            let pattern_chars: Vec<char> = pattern.chars().collect();
                            if offset < pattern_chars.len()
                                && Self::chars_match::<CASE_INSENSITIVE>(ch, pattern_chars[offset])
                            {
                                if offset + 1 == pattern_chars.len() {
                                    self.epsilon_closure(*next, &mut next_set);
                                } else {
                                    next_set
                                        .insert(ExtendedState::with_offset(nfa_state, offset + 1));
                                }
                            }
                        }
                    }
                }
                _ => {
                    // Other state types don't consume characters
                    // Anchors, Accept, Split, etc. are handled via epsilon_closure
                }
            }
        }

        let next_id = if next_set.is_empty() {
            TRANS_DEAD
        } else {
            self.get_or_create_state(next_set)
        };

        // Cache the transition
        if ch.is_ascii() {
            self.states[state_idx]
                .ascii_transitions
                .set(ch as u8, next_id);
            // For case-insensitive matching, also cache the other case variant
            if CASE_INSENSITIVE {
                let lower = ch.to_ascii_lowercase() as u8;
                let upper = ch.to_ascii_uppercase() as u8;
                if lower != ch as u8 {
                    self.states[state_idx].ascii_transitions.set(lower, next_id);
                }
                if upper != ch as u8 {
                    self.states[state_idx].ascii_transitions.set(upper, next_id);
                }
            }
        } else {
            self.states[state_idx]
                .unicode_transitions
                .insert(ch, next_id);
            // For case-insensitive non-ASCII, cache both variants
            if CASE_INSENSITIVE {
                for variant in ch.to_lowercase() {
                    if variant != ch {
                        self.states[state_idx]
                            .unicode_transitions
                            .insert(variant, next_id);
                    }
                }
                for variant in ch.to_uppercase() {
                    if variant != ch {
                        self.states[state_idx]
                            .unicode_transitions
                            .insert(variant, next_id);
                    }
                }
            }
        }

        if next_id == TRANS_DEAD {
            None
        } else {
            Some(next_id)
        }
    }

    /// Find the first match in the text.
    pub fn find(&mut self, text: &str) -> Option<DfaMatch> {
        // Fast path: character class plus patterns (\d+, \w+, \s+, [a-z]+)
        // Only use when similarity_threshold >= 1.0 (exact matching)
        if self.fast_path_char_class_plus {
            return self.find_char_class_plus(text);
        }

        // Fast path: simple literal pattern
        // Only use when similarity_threshold >= 1.0 (exact matching)
        if let Some(ref literal) = self.fast_path_literal {
            return self.find_literal(text, literal);
        }

        if self.anchored_start && !self.multi_line {
            // Only try at position 0 (non-multiline anchored pattern)
            self.find_at(text, 0)
        } else if self.anchored_start && self.multi_line {
            // In multiline mode, try at position 0 and after each newline
            self.find_multiline_anchored(text)
        } else {
            // Use prefilter to find candidate positions
            self.find_with_prefilter(text)
        }
    }

    /// Fast path for character classd+, \w plus patterns: \+, \s+, [a-z]+
    #[allow(dead_code)]
    fn find_char_class_plus(&self, text: &str) -> Option<DfaMatch> {
        let bytes = text.as_bytes();
        let len = bytes.len();

        if len == 0 {
            return None;
        }

        let matches_class = match self.fast_path_char_class_type {
            Some("digit") => |b: u8| b.is_ascii_digit(),
            Some("word") => |b: u8| b.is_ascii_alphanumeric() || b == b'_',
            Some("whitespace") => |b: u8| b.is_ascii_whitespace(),
            Some("not_digit") => |b: u8| !b.is_ascii_digit(),
            Some("not_word") => |b: u8| !b.is_ascii_alphanumeric() && b != b'_',
            Some("not_whitespace") => |b: u8| !b.is_ascii_whitespace(),
            _ => |b: u8| b.is_ascii_alphanumeric() || b == b'_',
        };

        let mut i = 0;
        while i < len {
            let start = i;
            while i < len && matches_class(bytes[i]) {
                i += 1;
            }

            if i > start {
                return Some(DfaMatch { start, end: i });
            }
            i += 1;
        }

        None
    }

    /// Fast path for simple literal patterns
    #[allow(dead_code, clippy::unused_self)]
    fn find_literal(&self, text: &str, literal: &str) -> Option<DfaMatch> {
        if literal.is_empty() {
            return Some(DfaMatch { start: 0, end: 0 });
        }

        let bytes = text.as_bytes();
        let lit_bytes = literal.as_bytes();
        let lit_len = lit_bytes.len();

        // Use TeddySearch for SIMD-accelerated forward search with rare byte optimization
        if bytes.len() >= 32 && lit_len >= 4 && lit_len <= 64 {
            let teddy = TeddySearch::new(lit_bytes);
            if let Some(pos) = teddy.find_first(bytes) {
                return Some(DfaMatch { start: pos, end: pos + lit_len });
            }
            return None;
        }

        // Fallback to memchr-based search for shorter patterns
        let first_byte = lit_bytes[0];
        let mut i = 0;
        while i <= bytes.len() - lit_len {
            if let Some(pos) = memchr(first_byte, &bytes[i..]) {
                let start = i + pos;
                if &bytes[start..start + lit_len] == lit_bytes {
                    return Some(DfaMatch {
                        start,
                        end: start + lit_len,
                    });
                }
                i = start + 1;
            } else {
                break;
            }
        }

        None
    }

    /// Fast path for simple literal patterns - reverse search
    /// Finds the rightmost occurrence of the literal using SIMD-accelerated search.
    #[allow(dead_code, clippy::unused_self)]
    fn find_literal_rev(&self, text: &str, literal: &str) -> Option<DfaMatch> {
        if literal.is_empty() {
            return Some(DfaMatch { start: text.len(), end: text.len() });
        }

        let bytes = text.as_bytes();
        let lit_bytes = literal.as_bytes();
        let lit_len = lit_bytes.len();

        // Use TeddySearch for SIMD-accelerated reverse search with rare byte optimization
        // Teddy is most effective for patterns >= 4 bytes where rare byte selection helps
        if bytes.len() >= 32 && lit_len >= 4 && lit_len <= 64 {
            let teddy = TeddySearch::new(lit_bytes);
            if let Some(pos) = teddy.find_last(bytes) {
                return Some(DfaMatch { start: pos, end: pos + lit_len });
            }
            return None;
        }

        // Use basic SIMD-accelerated reverse search for shorter patterns
        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            if bytes.len() >= 32 && lit_len <= 16 {
                if let Some(pos) = Self::find_literal_rev_simd(bytes, lit_bytes) {
                    return Some(DfaMatch { start: pos, end: pos + lit_len });
                }
                return None;
            }
        }

        // Fallback to memchr-based search
        let first_byte = lit_bytes[0];
        let mut pos = bytes.len();
        while pos >= lit_len {
            match memrchr(first_byte, &bytes[..pos]) {
                Some(start) => {
                    if start + lit_len <= bytes.len() && &bytes[start..start + lit_len] == lit_bytes {
                        return Some(DfaMatch { start, end: start + lit_len });
                    }
                    pos = start;
                }
                None => return None,
            }
        }
        None
    }

    /// SIMD-accelerated reverse literal search using AVX2.
    /// Scans from right to left, finding the rightmost occurrence.
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    #[allow(dead_code)]
    fn find_literal_rev_simd(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        use std::arch::x86_64::*;

        let len = haystack.len();
        let n = needle.len();

        if n == 0 {
            return Some(len);
        }
        if len < n {
            return None;
        }

        let ptr = haystack.as_ptr();
        let first_byte = needle[0] as i8;
        let v0 = _mm256_set1_epi8(first_byte);

        // Process 32-byte chunks from right to left
        if len >= 32 {
            let mut pos = len - 32;
            loop {
                let chunk = _mm256_loadu_si256(ptr.add(pos) as *const __m256i);
                let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, v0)) as u32;

                // Check additional needle bytes if present
                if n >= 2 {
                    let v1 = _mm256_set1_epi8(needle[1] as i8);
                    mask |= _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, v1)) as u32;
                }
                if n >= 3 {
                    let v2 = _mm256_set1_epi8(needle[2] as i8);
                    mask |= _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, v2)) as u32;
                }

                if mask != 0 {
                    // Find rightmost match using leading_zeros
                    let rightmost_pos = pos + 31 - mask.leading_zeros() as usize;
                    // Verify full literal match
                    if rightmost_pos + n <= len
                        && &haystack[rightmost_pos..rightmost_pos + n] == needle
                    {
                        return Some(rightmost_pos);
                    }
                }

                if pos < 32 {
                    break;
                }
                pos -= 32;
            }
        }

        // Handle remaining bytes at the start of the string
        if len < 32 {
            let mut buf = [0u8; 32];
            buf[..len].copy_from_slice(haystack);
            let chunk = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);
            let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, v0)) as u32;

            if n >= 2 {
                let v1 = _mm256_set1_epi8(needle[1] as i8);
                mask |= _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, v1)) as u32;
            }
            if n >= 3 {
                let v2 = _mm256_set1_epi8(needle[2] as i8);
                mask |= _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, v2)) as u32;
            }

            mask &= (1u32 << len) - 1;
            if mask != 0 {
                let rightmost_pos = 31 - mask.leading_zeros() as usize;
                if rightmost_pos + n <= len && &haystack[rightmost_pos..rightmost_pos + n] == needle {
                    return Some(rightmost_pos);
                }
            }
        }

        None
    }

    /// Find match for start-anchored patterns in multiline mode.
    /// Tries at position 0 and after each newline.
    fn find_multiline_anchored(&mut self, text: &str) -> Option<DfaMatch> {
        // Try at position 0
        if let Some(m) = self.find_at(text, 0) {
            return Some(m);
        }

        // Try after each newline
        let bytes = text.as_bytes();
        let mut offset = 0;
        while let Some(pos) = memchr(b'\n', &bytes[offset..]) {
            let start = offset + pos + 1; // Position after the newline
            if start < bytes.len()
                && let Some(m) = self.find_at(text, start)
            {
                return Some(m);
            }
            offset = start;
        }
        None
    }

    /// Find using prefilter to skip non-candidate positions.
    fn find_with_prefilter(&mut self, text: &str) -> Option<DfaMatch> {
        let bytes = text.as_bytes();
        let accepts_empty = self.states[self.start as usize].is_accept;

        // For patterns that can match empty strings:
        // - End-anchored patterns (like ^$) can only match empty if text is empty
        // - Other patterns should try to find a longer match first via find_at
        // We handle the empty-only case by trying find_at(0) first, which will
        // return the longest match starting at position 0.
        if accepts_empty && bytes.is_empty() {
            // Empty text with pattern that accepts empty - return empty match
            return Some(DfaMatch { start: 0, end: 0 });
        }

        // For non-empty text, try to find the longest match at position 0 first
        // This handles patterns like a* that should match "aaa" not ""
        if accepts_empty && !self.anchored_end {
            if let Some(m) = self.find_at(text, 0) {
                return Some(m);
            }
            // If no match at position 0, return empty match as fallback
            return Some(DfaMatch { start: 0, end: 0 });
        }

        // For end-anchored patterns on non-empty text, fall through to regular matching
        // (the end anchor check will reject empty matches at position 0)

        // Clone prefilter data to avoid borrow conflicts
        let prefilter = self.prefilter.clone();

        match prefilter {
            DfaPrefilter::None => {
                // No prefilter - scan every position
                self.find_linear(text)
            }
            DfaPrefilter::SingleByte(needle) => {
                let mut offset = 0;
                while let Some(pos) = memchr(needle, &bytes[offset..]) {
                    let start = offset + pos;
                    if let Some(m) = self.find_at(text, start) {
                        return Some(m);
                    }
                    // Move past this position
                    offset = start + 1;
                }
                // Handle patterns that accept empty string
                if accepts_empty {
                    return Some(DfaMatch { start: 0, end: 0 });
                }
                None
            }
            DfaPrefilter::TwoBytes(needle1, needle2) => {
                let mut offset = 0;
                while let Some(pos) = memchr::memchr2(needle1, needle2, &bytes[offset..]) {
                    let start = offset + pos;
                    if let Some(m) = self.find_at(text, start) {
                        return Some(m);
                    }
                    offset = start + 1;
                }
                if accepts_empty {
                    return Some(DfaMatch { start: 0, end: 0 });
                }
                None
            }
            DfaPrefilter::ThreeBytes(needle1, needle2, needle3) => {
                let mut offset = 0;
                while let Some(pos) = memchr::memchr3(needle1, needle2, needle3, &bytes[offset..]) {
                    let start = offset + pos;
                    if let Some(m) = self.find_at(text, start) {
                        return Some(m);
                    }
                    offset = start + 1;
                }
                if accepts_empty {
                    return Some(DfaMatch { start: 0, end: 0 });
                }
                None
            }
            DfaPrefilter::ManyBytes(ref needles) => {
                // Create a byte set for O(1) lookup
                let mut byte_set = [false; 256];
                for &b in needles {
                    byte_set[b as usize] = true;
                }
                let mut offset = 0;
                while offset < bytes.len() {
                    // Find next byte in our set
                    if let Some(pos) = bytes[offset..].iter().position(|&b| byte_set[b as usize]) {
                        let start = offset + pos;
                        if let Some(m) = self.find_at(text, start) {
                            return Some(m);
                        }
                        offset = start + 1;
                    } else {
                        break;
                    }
                }
                if accepts_empty {
                    return Some(DfaMatch { start: 0, end: 0 });
                }
                None
            }
            DfaPrefilter::Literal(ref lit) => {
                let finder = memmem::Finder::new(lit);
                let mut offset = 0;
                while let Some(pos) = finder.find(&bytes[offset..]) {
                    let start = offset + pos;
                    if let Some(m) = self.find_at(text, start) {
                        return Some(m);
                    }
                    offset = start + 1;
                }
                if accepts_empty {
                    return Some(DfaMatch { start: 0, end: 0 });
                }
                None
            }
            DfaPrefilter::CharClassRanges(ref ranges) => {
                use super::simd_class::RevSearchRanges;
                let searcher = RevSearchRanges::new(ranges.clone());
                let mut offset = 0;
                while let Some(pos) = searcher.find_first(&bytes[offset..]) {
                    let start = offset + pos;
                    if let Some(m) = self.find_at(text, start) {
                        return Some(m);
                    }
                    offset = start + 1;
                }
                None
            }
        }
    }

    /// Skip-accelerated search using DFA-level interesting bytes.
    /// Instead of checking every position, we only check positions where
    /// the byte causes a non-self-loop transition in the DFA.
    fn find_with_skip_acceleration_offset(&mut self, text: &str, candidates: &[u8], start_offset: usize) -> Option<DfaMatch> {
        let bytes = text.as_bytes();
        let len = bytes.len();
        
        if len <= start_offset {
            return None;
        }
        
        let search_bytes = &bytes[start_offset..];
        
        // For single candidate byte, use memchr
        if candidates.len() == 1 {
            let needle = candidates[0];
            let mut offset = 0;
            while let Some(pos) = memchr(needle, &search_bytes[offset..]) {
                let global_pos = start_offset + offset + pos;
                if let Some(m) = self.find_at(text, global_pos) {
                    return Some(m);
                }
                offset = offset + pos + 1;
            }
            return None;
        }

        // For 2 candidates, use memchr2
        if candidates.len() == 2 {
            let (b1, b2) = (candidates[0], candidates[1]);
            let mut offset = 0;
            while let Some(pos) = memchr::memchr2(b1, b2, &search_bytes[offset..]) {
                let global_pos = start_offset + offset + pos;
                if let Some(m) = self.find_at(text, global_pos) {
                    return Some(m);
                }
                offset = offset + pos + 1;
            }
            return None;
        }

        // For 3 candidates, use memchr3
        if candidates.len() == 3 {
            let (b1, b2, b3) = (candidates[0], candidates[1], candidates[2]);
            let mut offset = 0;
            while let Some(pos) = memchr::memchr3(b1, b2, b3, &search_bytes[offset..]) {
                let global_pos = start_offset + offset + pos;
                if let Some(m) = self.find_at(text, global_pos) {
                    return Some(m);
                }
                offset = offset + pos + 1;
            }
            return None;
        }

        // For 4+ candidates, use a byte set lookup
        let mut byte_set = [false; 256];
        for &b in candidates {
            byte_set[b as usize] = true;
        }
        
        let mut offset = start_offset;
        while offset < len {
            if byte_set[bytes[offset] as usize] {
                if let Some(m) = self.find_at(text, offset) {
                    return Some(m);
                }
            }
            offset += 1;
        }
        None
    }

    /// Skip-accelerated search using DFA-level interesting bytes.
    /// Instead of checking every position, we only check positions where
    /// the byte causes a non-self-loop transition in the DFA.
    #[allow(dead_code)]
    fn find_with_skip_acceleration(&mut self, text: &str, candidates: &[u8]) -> Option<DfaMatch> {
        self.find_with_skip_acceleration_offset(text, candidates, 0)
    }

    /// Linear scan through all positions (fallback when no prefilter).
    /// Note: Skip acceleration is disabled because it requires a complete DFA
    /// with populated transitions, which lazy DFA doesn't have.
    fn find_linear(&mut self, text: &str) -> Option<DfaMatch> {
        for (start_pos, _) in text.char_indices() {
            if let Some(m) = self.find_at(text, start_pos) {
                return Some(m);
            }
        }
        // Try at end for patterns that match empty string
        self.find_at(text, text.len())
    }

    /// Find a match starting at a specific position.
    /// Dispatches to const generic implementation for branch elimination.
    #[inline(always)]
    pub fn find_at(&mut self, text: &str, start: usize) -> Option<DfaMatch> {
        if self.multi_line {
            self.find_at_impl::<true>(text, start)
        } else {
            self.find_at_impl::<false>(text, start)
        }
    }

    /// Const generic implementation of `find_at`.
    /// `MULTI_LINE` is a compile-time constant, eliminating runtime branches.
    #[inline(always)]
    fn find_at_impl<const MULTI_LINE: bool>(
        &mut self,
        text: &str,
        start: usize,
    ) -> Option<DfaMatch> {
        let bytes = text.as_bytes();
        let mut state_id = self.start;
        let mut last_accept: Option<usize> = None;

        // Check if start position satisfies start anchor (^ constraint)
        let start_anchor_ok = Self::is_start_anchor_satisfied::<MULTI_LINE>(bytes, start);

        // Check if start state is accepting
        if self.states[state_id as usize].is_accept {
            // For anchored_start patterns, check the start anchor
            if !self.anchored_start || start_anchor_ok {
                // For end-anchored patterns, also check the end anchor
                let end_anchor_ok = Self::is_end_anchor_satisfied::<MULTI_LINE>(bytes, start);
                if !self.anchored_end || end_anchor_ok {
                    last_accept = Some(start);
                }
            }
        }

        // Handle start anchor - must be satisfied
        if self.states[state_id as usize].has_start_anchor && !start_anchor_ok {
            return None;
        }

        let mut pos = start;
        for ch in text[start..].chars() {
            // Compute next state
            let next = self.next_state(state_id, ch);

            match next {
                Some(next_id) => {
                    state_id = next_id;
                    pos += ch.len_utf8();

                    // Check if this is an accepting state - track it for longest match
                    if self.states[state_id as usize].is_accept {
                        // For end-anchored patterns, check end anchor constraint
                        let end_anchor_ok = Self::is_end_anchor_satisfied::<MULTI_LINE>(bytes, pos);
                        if !self.states[state_id as usize].has_end_anchor || end_anchor_ok {
                            last_accept = Some(pos);
                        }
                    }
                }
                None => {
                    // Dead state - return last accepting position if any
                    break;
                }
            }
        }

        // Check for end anchor at final position
        if self.states[state_id as usize].has_end_anchor {
            let end_anchor_ok = Self::is_end_anchor_satisfied::<MULTI_LINE>(bytes, pos);
            if end_anchor_ok && self.states[state_id as usize].is_accept {
                last_accept = Some(pos);
            }
        }

        last_accept.map(|end| DfaMatch { start, end })
    }

    /// Check if start anchor (^) is satisfied at the given position.
    /// Uses const generic to eliminate multiline branch at compile time.
    #[inline(always)]
    fn is_start_anchor_satisfied<const MULTI_LINE: bool>(bytes: &[u8], pos: usize) -> bool {
        if pos == 0 {
            return true;
        }
        if MULTI_LINE && bytes[pos - 1] == b'\n' {
            return true;
        }
        false
    }

    /// Check if end anchor ($) is satisfied at the given position.
    /// Uses const generic to eliminate multiline branch at compile time.
    #[inline(always)]
    fn is_end_anchor_satisfied<const MULTI_LINE: bool>(bytes: &[u8], pos: usize) -> bool {
        if pos == bytes.len() {
            return true;
        }
        if MULTI_LINE && bytes[pos] == b'\n' {
            return true;
        }
        false
    }

    /// Find all non-overlapping matches.
    pub fn find_all(&mut self, text: &str) -> Vec<DfaMatch> {
        let mut matches = Vec::new();
        let mut pos = 0;

        while pos <= text.len() {
            if let Some(m) = self.find_at(text, pos) {
                let end = m.end;
                matches.push(m);
                // Move past this match
                pos = if end > pos {
                    end
                } else {
                    // Empty match - advance by one character
                    text[pos..]
                        .chars()
                        .next()
                        .map_or(text.len() + 1, |c| pos + c.len_utf8())
                };
            } else {
                // No match at this position - advance
                pos = text[pos..]
                    .chars()
                    .next()
                    .map_or(text.len() + 1, |c| pos + c.len_utf8());
            }
        }

        matches
    }

    /// Find the rightmost match (reverse search).
    /// More efficient than finding all matches and taking the last.
    /// 
    /// Scans from right to left using dynamic programming to track possible states.
    /// Time complexity: O(n × s) where n = text length, s = number of states.
    /// This is much faster than O(n × m) for texts with many matches.
    pub fn find_rev(&mut self, text: &str) -> Option<DfaMatch> {
        let bytes = text.as_bytes();
        let len = bytes.len();
        
        if len == 0 {
            // Check for empty match at position 0
            if self.states[self.start as usize].is_accept {
                return Some(DfaMatch { start: 0, end: 0 });
            }
            return None;
        }

        // For end-anchored patterns, we can use a more efficient approach
        // Try to match ending at the end of the string first
        if self.anchored_end {
            // Scan backwards from end, looking for a match that ends at the final position
            for start_pos in (0..=len).rev() {
                if let Some(m) = self.find_at(text, start_pos) {
                    // Check if this match ends at the final position
                    if m.end == len {
                        return Some(m);
                    }
                    // If match ends before final position, no need to continue
                    if m.end < len {
                        return None;
                    }
                }
            }
            return None;
        }

        // Fast path: use reverse literal search if we have a simple literal
        // For exact literal patterns, the literal match IS the DFA match
        if let Some(ref literal) = self.fast_path_literal {
            return self.find_literal_rev(text, literal);
        }

        // For non-anchored patterns: use efficient right-to-left scanning
        // This is faster than find_all when there are many matches
        self.find_rev_with_prefilter(text)
    }

    /// Find rightmost match using prefilter to scan from right to left.
    /// Uses prefilter to find candidate positions, then verifies from right to left.
    fn find_rev_with_prefilter(&mut self, text: &str) -> Option<DfaMatch> {
        let bytes = text.as_bytes();
        let len = bytes.len();

        if len == 0 {
            if self.states[self.start as usize].is_accept {
                return Some(DfaMatch { start: 0, end: 0 });
            }
            return None;
        }

        // Clone prefilter to avoid borrow issues
        let prefilter = self.prefilter.clone();

        match prefilter {
            DfaPrefilter::None => {
                // No prefilter - fall back to scanning all positions
                self.find_all(text).into_iter().last()
            }
            DfaPrefilter::SingleByte(needle) => {
                // Scan from right using memrchr
                let mut pos = len;
                while pos > 0 {
                    if let Some(found) = memrchr(needle, &bytes[..pos]) {
                        if let Some(m) = self.find_at(text, found) {
                            return Some(m);
                        }
                        pos = found;
                    } else {
                        break;
                    }
                }
                None
            }
            DfaPrefilter::TwoBytes(b1, b2) => {
                // Scan from right - find rightmost occurrence of either byte
                let mut pos = len;
                while pos > 0 {
                    // Find rightmost occurrence of b1 or b2
                    let found_b1 = memrchr(b1, &bytes[..pos]);
                    let found_b2 = memrchr(b2, &bytes[..pos]);
                    
                    // Use the rightmost one
                    let found = match (found_b1, found_b2) {
                        (Some(p1), Some(p2)) => Some(p1.max(p2)),
                        (Some(p), None) => Some(p),
                        (None, Some(p)) => Some(p),
                        (None, None) => None,
                    };
                    
                    match found {
                        Some(found) => {
                            if let Some(m) = self.find_at(text, found) {
                                return Some(m);
                            }
                            pos = found;
                        }
                        None => break,
                    }
                }
                None
            }
            DfaPrefilter::ThreeBytes(b1, b2, b3) => {
                // Scan from right - find rightmost occurrence of any byte
                let mut pos = len;
                while pos > 0 {
                    let found_b1 = memrchr(b1, &bytes[..pos]);
                    let found_b2 = memrchr(b2, &bytes[..pos]);
                    let found_b3 = memrchr(b3, &bytes[..pos]);
                    
                    let found = [found_b1, found_b2, found_b3]
                        .into_iter()
                        .flatten()
                        .max();
                    
                    match found {
                        Some(found) => {
                            if let Some(m) = self.find_at(text, found) {
                                return Some(m);
                            }
                            pos = found;
                        }
                        None => break,
                    }
                }
                None
            }
            DfaPrefilter::CharClassRanges(ref ranges) => {
                let searcher = RevSearchRanges::new(ranges.clone());
                let mut pos = len;
                while pos > 0 {
                    if let Some(found) = searcher.find_last(&bytes[..pos]) {
                        // For character class patterns, we need to scan backwards
                        // to find the start of the match, not just any byte in the class.
                        // For example, for \d+ in "abc123def456", finding '6' at pos 11
                        // should give us the match starting at '4' (pos 9).
                        let match_start = if found > 0 && searcher.matches_byte(bytes[found - 1]) {
                            // Scan backwards through consecutive matching bytes
                            let mut start = found;
                            while start > 0 && searcher.matches_byte(bytes[start - 1]) {
                                start -= 1;
                            }
                            start
                        } else {
                            found
                        };
                        
                        if let Some(m) = self.find_at(text, match_start) {
                            return Some(m);
                        }
                        pos = found;
                    } else {
                        break;
                    }
                }
                None
            }
            _ => {
                // For other prefilters, fall back to find_all
                self.find_all(text).into_iter().last()
            }
        }
    }

    /// Find the first `n` non-overlapping matches.
    pub fn find_n(&mut self, text: &str, n: usize) -> Vec<DfaMatch> {
        if n == 0 {
            return Vec::new();
        }

        let mut matches = Vec::with_capacity(n);
        let mut pos = 0;

        while pos <= text.len() && matches.len() < n {
            if let Some(m) = self.find_at(text, pos) {
                let end = m.end;
                matches.push(m);
                // Move past this match
                pos = if end > pos {
                    end
                } else {
                    // Empty match - advance by one character
                    text[pos..]
                        .chars()
                        .next()
                        .map_or(text.len() + 1, |c| pos + c.len_utf8())
                };
            } else {
                // No match at this position - advance
                pos = text[pos..]
                    .chars()
                    .next()
                    .map_or(text.len() + 1, |c| pos + c.len_utf8());
            }
        }

        matches
    }

    /// Check if the DFA is anchored at start.
    #[must_use]
    pub fn is_anchored_start(&self) -> bool {
        self.anchored_start
    }

    /// Check if the DFA is anchored at end.
    #[must_use]
    pub fn is_anchored_end(&self) -> bool {
        self.anchored_end
    }

    /// Get the number of states in the DFA.
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    /// Find the shortest match and return its end position.
    /// Returns None if no match is found.
    /// This is useful for prefix matching where you want to know how far the match extends.
    pub fn first_end(&mut self, text: &str) -> Option<usize> {
        self.find(text).map(|m| m.end)
    }

    /// Find the longest match starting at position 0 and return its end position.
    /// Returns None if no match is found.
    /// This is useful for full-string matching where you want the longest match.
    pub fn longest_end(&mut self, text: &str) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut state_id = self.start;
        let mut last_accept: Option<usize> = None;

        // Check if start position satisfies start anchor
        let start_anchor_ok = Self::is_start_anchor_satisfied::<false>(bytes, 0);

        // Check if start state is accepting
        if self.states[state_id as usize].is_accept
            && (!self.anchored_start || start_anchor_ok)
            && !self.anchored_end
        {
            last_accept = Some(0);
        }

        // Run the DFA
        let mut pos = 0;
        for ch in text.chars() {
            let next = self.next_state(state_id, ch);

            match next {
                Some(next_id) => {
                    state_id = next_id;
                    pos += ch.len_utf8();

                    // Track accepting states for longest match
                    if self.states[state_id as usize].is_accept {
                        let end_anchor_ok = Self::is_end_anchor_satisfied::<false>(bytes, pos);
                        if !self.states[state_id as usize].has_end_anchor || end_anchor_ok {
                            last_accept = Some(pos);
                        }
                    }
                }
                None => break,
            }
        }

        // Check end anchor at final position
        if self.states[state_id as usize].has_end_anchor {
            let end_anchor_ok = Self::is_end_anchor_satisfied::<false>(bytes, pos);
            if end_anchor_ok && self.states[state_id as usize].is_accept {
                last_accept = Some(pos);
            }
        }

        last_accept
    }

    /// Get the "interesting" bytes that cause non-self-loop transitions from the start state.
    /// These are bytes that the DFA actually processes instead of self-looping.
    /// Returns None if too many interesting bytes (not effective for skip acceleration).
    #[must_use]
    pub fn get_skip_candidates(&self) -> Option<Vec<u8>> {
        // Note: For lazy DFA, transitions may not be fully computed.
        // This method assumes transitions are already populated (DFA is complete).
        // Callers should ensure DFA is complete before calling this.
        
        let start_state = &self.states[self.start as usize];
        let mut interesting: Vec<u8> = Vec::with_capacity(128);
        
        // Find bytes that transition to a different state (not self-loop, not dead)
        for byte in 0u8..128 {
            let target = start_state.ascii_transitions.get(byte);
            if target != TRANS_UNKNOWN && target != TRANS_DEAD && target != self.start {
                interesting.push(byte);
            }
        }
        
        // If too many interesting bytes, skip acceleration won't help much
        if interesting.len() > 32 {
            None
        } else {
            Some(interesting)
        }
    }

    /// Complete the DFA by computing all reachable transitions.
    /// This is required before minimization or for full DFA precompilation.
    pub fn complete(&mut self) {
        // Compute all transitions for all states by iterating over ASCII chars
        // We iterate until no new states are discovered
        let mut i = 0;
        while i < self.states.len() {
            // Compute transitions for all printable ASCII characters
            for byte in 0u8..128 {
                let ch = byte as char;
                let _ = self.next_state(i as DfaStateId, ch);
            }
            i += 1;
        }
    }

    /// Minimize the DFA using Hopcroft's algorithm.
    /// Returns the number of states removed.
    #[allow(clippy::mut_range_bound)] // Intentional: num_partitions grows during iteration
    #[allow(clippy::needless_range_loop)] // Clearer to use index for partition mapping
    pub fn minimize(&mut self) -> usize {
        let original_count = self.states.len();
        if original_count <= 1 {
            return 0;
        }

        // First, complete the DFA to ensure all transitions are computed
        self.complete();

        // Collect all characters that have transitions
        let mut alphabet: Vec<char> = Vec::new();
        for byte in 0u8..128 {
            alphabet.push(byte as char);
        }
        // Also collect non-ASCII characters from unicode_transitions
        for state in &self.states {
            for &ch in state.unicode_transitions.keys() {
                if !alphabet.contains(&ch) {
                    alphabet.push(ch);
                }
            }
        }

        // Initialize partitions: separate accepting from non-accepting states
        // Also consider anchor states as different equivalence classes
        let mut partition: Vec<usize> = vec![0; self.states.len()];
        let mut num_partitions = 0;

        // Create initial partitions based on (is_accept, has_start_anchor, has_end_anchor)
        let mut partition_map: HashMap<(bool, bool, bool), usize> = HashMap::new();
        for (i, state) in self.states.iter().enumerate() {
            let key = (
                state.is_accept,
                state.has_start_anchor,
                state.has_end_anchor,
            );
            let p = *partition_map.entry(key).or_insert_with(|| {
                let p = num_partitions;
                num_partitions += 1;
                p
            });
            partition[i] = p;
        }

        // Hopcroft's algorithm: refine partitions until stable
        let mut changed = true;
        while changed {
            changed = false;

            // For each partition, check if it needs to be split
            for p in 0..num_partitions {
                // Get states in this partition
                let states_in_p: Vec<usize> = partition
                    .iter()
                    .enumerate()
                    .filter(|&(_, part)| *part == p)
                    .map(|(i, _)| i)
                    .collect();

                if states_in_p.len() <= 1 {
                    continue;
                }

                // Check if all states in this partition have the same behavior
                // Two states are equivalent if for all input symbols,
                // they transition to states in the same partition
                let first = states_in_p[0];
                let mut to_split: Vec<usize> = Vec::new();

                for &state_idx in &states_in_p[1..] {
                    let mut same = true;
                    for &ch in &alphabet {
                        let t1 = self.get_transition(first, ch);
                        let t2 = self.get_transition(state_idx, ch);

                        let p1 = t1.map(|s| partition[s as usize]);
                        let p2 = t2.map(|s| partition[s as usize]);

                        if p1 != p2 {
                            same = false;
                            break;
                        }
                    }
                    if !same {
                        to_split.push(state_idx);
                    }
                }

                // Split the partition if needed
                if !to_split.is_empty() {
                    let new_partition = num_partitions;
                    num_partitions += 1;
                    for &state_idx in &to_split {
                        partition[state_idx] = new_partition;
                    }
                    changed = true;
                }
            }
        }

        // Check if any minimization is possible
        if num_partitions == original_count {
            return 0;
        }

        // Build the minimized DFA
        // Each partition becomes a new state
        let mut new_states: Vec<DfaState> = Vec::with_capacity(num_partitions);
        let mut old_to_new: Vec<DfaStateId> = vec![0; original_count];

        // Create new states (one per partition)
        for p in 0..num_partitions {
            // Find a representative state for this partition
            let repr = partition
                .iter()
                .enumerate()
                .find(|&(_, part)| *part == p)
                .map(|(i, _)| i)
                .unwrap();

            old_to_new[repr] = p as DfaStateId;

            let old_state = &self.states[repr];
            new_states.push(DfaState {
                nfa_states: old_state.nfa_states.clone(),
                is_accept: old_state.is_accept,
                has_start_anchor: old_state.has_start_anchor,
                has_end_anchor: old_state.has_end_anchor,
                ascii_transitions: AsciiTransitions::new(),
                unicode_transitions: FxHashMap::default(),
            });
        }

        // Map all old states to their new partition IDs
        for (i, &p) in partition.iter().enumerate() {
            old_to_new[i] = p as DfaStateId;
        }

        // Remap transitions
        for p in 0..num_partitions {
            // Find representative for this partition
            let repr = partition
                .iter()
                .enumerate()
                .find(|&(_, part)| *part == p)
                .map(|(i, _)| i)
                .unwrap();

            let old_state = &self.states[repr];

            // Remap ASCII transitions
            for byte in 0u8..128 {
                let old_target = old_state.ascii_transitions.get(byte);
                let new_target = if old_target == TRANS_UNKNOWN || old_target == TRANS_DEAD {
                    old_target
                } else {
                    old_to_new[old_target as usize]
                };
                new_states[p].ascii_transitions.set(byte, new_target);
            }

            // Remap unicode transitions
            for (&ch, &old_target) in &old_state.unicode_transitions {
                let new_target = if old_target == TRANS_DEAD {
                    old_target
                } else {
                    old_to_new[old_target as usize]
                };
                new_states[p].unicode_transitions.insert(ch, new_target);
            }
        }

        // Update the DFA
        let new_start = old_to_new[self.start as usize];
        self.states = new_states;
        self.start = new_start;

        // Rebuild state cache
        self.state_cache.clear();
        for (i, state) in self.states.iter().enumerate() {
            self.state_cache
                .insert(state.nfa_states.clone(), i as DfaStateId);
        }

        original_count - self.states.len()
    }

    /// Get transition for a state and character (without computing new transitions).
    fn get_transition(&self, state_idx: usize, ch: char) -> Option<DfaStateId> {
        let state = &self.states[state_idx];
        if ch.is_ascii() {
            let cached = state.ascii_transitions.get(ch as u8);
            if cached == TRANS_UNKNOWN || cached == TRANS_DEAD {
                None
            } else {
                Some(cached)
            }
        } else {
            state
                .unicode_transitions
                .get(&ch)
                .copied()
                .filter(|&t| t != TRANS_DEAD)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::build_nfa;
    use crate::ir::lower;
    use crate::parser::parse;

    fn make_dfa(pattern: &str) -> Option<Dfa> {
        make_dfa_ci(pattern, false)
    }

    fn make_dfa_ci(pattern: &str, case_insensitive: bool) -> Option<Dfa> {
        make_dfa_full(pattern, case_insensitive, false)
    }

    fn make_dfa_full(pattern: &str, case_insensitive: bool, multi_line: bool) -> Option<Dfa> {
        let ast = parse(pattern).unwrap();
        let hir = lower(&ast, 0);
        let (nfa, literals) = build_nfa(&hir);

        // Create FuzzyBridge if we have literals
        let bridge = if literals.is_empty() {
            None
        } else {
            FuzzyBridge::new(&literals, None, None, case_insensitive)
        };

        Dfa::from_nfa(&nfa, bridge.as_ref(), case_insensitive, multi_line, 1.0)
    }

    #[test]
    fn test_simple_literal() {
        let mut dfa = make_dfa("hello").unwrap();
        let m = dfa.find("hello world").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);

        let m = dfa.find("say hello").unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 9);
    }

    #[test]
    fn test_char_class() {
        let mut dfa = make_dfa("[a-z]+").unwrap();
        let m = dfa.find("123abc456").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_start_anchor() {
        let mut dfa = make_dfa("^hello").unwrap();
        assert!(dfa.find("hello world").is_some());
        assert!(dfa.find("say hello").is_none());
    }

    #[test]
    fn test_end_anchor() {
        let mut dfa = make_dfa("world$").unwrap();
        let m = dfa.find("hello world").unwrap();
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 11);

        assert!(dfa.find("world hello").is_none());
    }

    #[test]
    fn test_alternation() {
        let mut dfa = make_dfa("cat|dog").unwrap();
        assert!(dfa.find("I have a cat").is_some());
        assert!(dfa.find("I have a dog").is_some());
        assert!(dfa.find("I have a bird").is_none());
    }

    #[test]
    fn test_quantifiers() {
        let mut dfa = make_dfa("a+").unwrap();
        let m = dfa.find("baaab").unwrap();
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 4);

        let mut dfa = make_dfa("a*").unwrap();
        let m = dfa.find("baaab").unwrap();
        assert_eq!(m.start, 0); // Empty match at start
        assert_eq!(m.end, 0);

        let mut dfa = make_dfa("a?").unwrap();
        let m = dfa.find("baaab").unwrap();
        assert_eq!(m.start, 0); // Empty match at start
    }

    #[test]
    fn test_find_all() {
        let mut dfa = make_dfa("[a-z]+").unwrap();
        let matches = dfa.find_all("abc 123 def 456 ghi");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 3);
        assert_eq!(matches[1].start, 8);
        assert_eq!(matches[1].end, 11);
    }

    #[test]
    fn test_fuzzy_not_compatible() {
        // Fuzzy patterns should return None
        assert!(make_dfa("(?:hello){e<=1}").is_none());
        assert!(make_dfa("hello~1").is_none());
    }

    // Case-insensitive tests
    #[test]
    fn test_case_insensitive_literal() {
        let mut dfa = make_dfa_ci("hello", true).unwrap();

        // Should match all case variants
        assert!(dfa.find("hello").is_some());
        assert!(dfa.find("HELLO").is_some());
        assert!(dfa.find("HeLLo").is_some());
        assert!(dfa.find("hElLo").is_some());

        // Should find in mixed text
        let m = dfa.find("say HELLO world").unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 9);
    }

    #[test]
    fn test_case_insensitive_char_class() {
        let mut dfa = make_dfa_ci("[a-z]+", true).unwrap();

        // Should match uppercase too
        let m = dfa.find("123ABC456").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);

        // Mixed case
        let m = dfa.find("123AbC456").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_case_insensitive_find_all() {
        let mut dfa = make_dfa_ci("hello", true).unwrap();
        let matches = dfa.find_all("hello HELLO HeLLo");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_case_sensitive_does_not_match_wrong_case() {
        let mut dfa = make_dfa_ci("hello", false).unwrap();

        assert!(dfa.find("hello").is_some());
        assert!(dfa.find("HELLO").is_none());
        assert!(dfa.find("HeLLo").is_none());
    }

    // Minimization tests
    #[test]
    fn test_minimize_simple() {
        let mut dfa = make_dfa("hello").unwrap();
        let before = dfa.state_count();
        let removed = dfa.minimize();
        let after = dfa.state_count();

        assert_eq!(removed, before - after);

        // Should still match correctly after minimization
        assert!(dfa.find("hello").is_some());
        assert!(dfa.find("world").is_none());
        let m = dfa.find("say hello").unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 9);
    }

    #[test]
    fn test_minimize_alternation() {
        // Alternation often creates mergeable states
        let mut dfa = make_dfa("cat|car|cap").unwrap();
        // Complete the DFA first to see all states
        dfa.complete();
        let before = dfa.state_count();
        let removed = dfa.minimize();
        let after = dfa.state_count();

        // States sharing "ca" prefix should be merged
        println!("cat|car|cap: Before={before}, After={after}, Removed={removed}");

        // Should still match correctly
        assert!(dfa.find("cat").is_some());
        assert!(dfa.find("car").is_some());
        assert!(dfa.find("cap").is_some());
        assert!(dfa.find("cab").is_none());
    }

    #[test]
    fn test_minimize_char_class() {
        let mut dfa = make_dfa("[abc]+").unwrap();
        // Complete the DFA first to see all states
        dfa.complete();
        let before = dfa.state_count();
        let removed = dfa.minimize();
        let after = dfa.state_count();

        println!("[abc]+ states: Before={before}, After={after}, Removed={removed}");

        // Should still work correctly
        let m = dfa.find("xyzabc123").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_minimize_preserves_anchors() {
        let mut dfa = make_dfa("^hello$").unwrap();
        dfa.minimize();

        // Anchors should still work
        assert!(dfa.find("hello").is_some());
        assert!(dfa.find("hello world").is_none()); // $ anchor
        assert!(dfa.find("say hello").is_none()); // ^ anchor
    }

    #[test]
    fn test_minimize_case_insensitive() {
        let mut dfa = make_dfa_ci("hello", true).unwrap();
        dfa.complete();
        let before = dfa.state_count();
        let removed = dfa.minimize();
        let after = dfa.state_count();

        println!("CI 'hello' states: Before={before}, After={after}, Removed={removed}");

        // Should still match case-insensitively
        assert!(dfa.find("hello").is_some());
        assert!(dfa.find("HELLO").is_some());
        assert!(dfa.find("HeLLo").is_some());
    }

    #[test]
    fn test_state_count() {
        let dfa = make_dfa("a").unwrap();
        assert!(dfa.state_count() >= 1);

        let dfa = make_dfa("abc").unwrap();
        assert!(dfa.state_count() >= 1);
    }

    #[test]
    fn test_minimize_with_bounded_quantifier() {
        // Pattern with bounded quantifier
        let mut dfa = make_dfa("^a{1,3}b$").unwrap();
        dfa.complete();
        let before = dfa.state_count();
        let removed = dfa.minimize();
        let after = dfa.state_count();

        println!("^a{{1,3}}b$: Before={before}, After={after}, Removed={removed}");

        // Should still match correctly with anchors
        assert!(dfa.find("ab").is_some());
        assert!(dfa.find("aab").is_some());
        assert!(dfa.find("aaab").is_some());
        assert!(dfa.find("aaaab").is_none()); // 4 a's - doesn't match
        assert!(dfa.find("b").is_none()); // no a's - doesn't match
    }

    #[test]
    fn test_minimize_complex_alternation() {
        // Many alternations might create some equivalent states
        let mut dfa = make_dfa("abc|abd|abe|abf").unwrap();
        dfa.complete();
        let before = dfa.state_count();
        let removed = dfa.minimize();
        let after = dfa.state_count();

        println!("abc|abd|abe|abf: Before={before}, After={after}, Removed={removed}");

        // Should still match
        assert!(dfa.find("abc").is_some());
        assert!(dfa.find("abd").is_some());
        assert!(dfa.find("abe").is_some());
        assert!(dfa.find("abf").is_some());
        assert!(dfa.find("abg").is_none());
    }

    // Multiline tests
    #[test]
    fn test_multiline_start_anchor() {
        // Without multiline: ^ only matches at position 0
        let mut dfa = make_dfa_full("^line", false, false).unwrap();
        assert!(dfa.find("line1\nline2").is_some());
        assert!(dfa.find("first\nline2").is_none()); // ^ doesn't match after \n

        // With multiline: ^ matches at start OR after \n
        let mut dfa_ml = make_dfa_full("^line", false, true).unwrap();
        assert!(dfa_ml.find("line1\nline2").is_some());
        let m = dfa_ml.find("first\nline2").unwrap();
        assert_eq!(m.start, 6); // matches "line" after newline
        assert_eq!(m.end, 10);
    }

    #[test]
    fn test_multiline_end_anchor() {
        // Without multiline: $ only matches at end of string
        let mut dfa = make_dfa_full("end$", false, false).unwrap();
        assert!(dfa.find("the end").is_some());
        assert!(dfa.find("end\nnext").is_none()); // $ doesn't match before \n

        // With multiline: $ matches at end OR before \n
        let mut dfa_ml = make_dfa_full("end$", false, true).unwrap();
        assert!(dfa_ml.find("the end").is_some());
        let m = dfa_ml.find("end\nnext").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn test_multiline_both_anchors() {
        // Pattern ^line$ should match whole lines in multiline mode
        let mut dfa_ml = make_dfa_full("^line$", false, true).unwrap();

        // Match first line
        let m = dfa_ml.find("line\nother").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 4);

        // Match middle line
        let m = dfa_ml.find("first\nline\nlast").unwrap();
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 10);

        // Match last line
        let m = dfa_ml.find("other\nline").unwrap();
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 10);
    }

    #[test]
    fn test_multiline_find_all() {
        let mut dfa_ml = make_dfa_full("^[a-z]+$", false, true).unwrap();
        let matches = dfa_ml.find_all("hello\n123\nworld");

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 5);
        assert_eq!(matches[1].start, 10);
        assert_eq!(matches[1].end, 15);
    }

    #[test]
    fn test_bytes_to_ranges() {
        assert_eq!(Dfa::bytes_to_ranges(&[]), Vec::<(u8, u8)>::new());
        assert_eq!(Dfa::bytes_to_ranges(&[5]), vec![(5, 5)]);
        assert_eq!(Dfa::bytes_to_ranges(&[1, 2, 3]), vec![(1, 3)]);
        assert_eq!(Dfa::bytes_to_ranges(&[1, 3, 5]), vec![(1, 1), (3, 3), (5, 5)]);
        assert_eq!(Dfa::bytes_to_ranges(&[1, 2, 5, 6, 9]), vec![(1, 2), (5, 6), (9, 9)]);
        assert_eq!(Dfa::bytes_to_ranges(&[b'0', b'1', b'2', b'5', b'6', b'7']), vec![(b'0', b'2'), (b'5', b'7')]);
    }

    #[test]
    fn test_char_class_ranges_prefilter() {
        // Test digit class - should use CharClassRanges
        let mut dfa = make_dfa(r"\d+").unwrap();
        let m = dfa.find("abc123def").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);

        // Test rightmost match using find_rev
        let m = dfa.find_rev("abc123def456").unwrap();
        assert_eq!(m.start, 9);
        assert_eq!(m.end, 12);

        // Test word class
        let mut dfa_word = make_dfa(r"\w+").unwrap();
        let m = dfa_word.find("hello123").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 8);
    }

    #[test]
    fn test_find_rev_with_char_class_ranges() {
        // Test rightmost match for character class patterns
        let mut dfa = make_dfa(r"\d+").unwrap();
        let m = dfa.find_rev("abc123def456ghi").unwrap();
        assert_eq!(m.start, 9);
        assert_eq!(m.end, 12);

        // Test multiple digit groups
        let m = dfa.find_rev("a1b22c333d").unwrap();
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 9);
    }

    #[test]
    fn test_teddy_literal_search() {
        // Test with longer literal patterns that use Teddy search
        let mut dfa = make_dfa("example").unwrap();
        let m = dfa.find("this is an example text").unwrap();
        assert_eq!(m.start, 11);
        assert_eq!(m.end, 18);

        // Test rightmost match with Teddy
        let m = dfa.find_rev("an example of example").unwrap();
        assert_eq!(m.start, 14);
        assert_eq!(m.end, 21);

        // Test with even longer pattern: "the quick brown fox" = 19 chars
        let mut dfa2 = make_dfa("the quick brown fox").unwrap();
        // "hello the quick brown fox world" - "the" starts at 6, pattern ends at 25
        let m = dfa2.find("hello the quick brown fox world").unwrap();
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 25);

        // "the quick brown fox and the quick brown fox"
        // First starts at 0, second starts at 24
        let m = dfa2.find_rev("the quick brown fox and the quick brown fox").unwrap();
        assert_eq!(m.start, 24);
        assert_eq!(m.end, 43);
    }

    // -- RE# Edge Case Tests --

    #[test]
    fn test_rev_search_at_every_position() {
        // Single byte search at every position
        for pos in 0..64 {
            let mut hay = vec![b'.'; 64];
            hay[pos] = b'Z';
            let mut dfa = make_dfa("Z").unwrap();
            let result = dfa.find(&String::from_utf8_lossy(&hay));
            assert!(result.is_some(), "pos={}", pos);
            assert_eq!(result.unwrap().start, pos, "pos={}", pos);
        }
    }

    #[test]
    fn test_rev_search_two_bytes_sweep() {
        // Test reverse search with byte class [XY]
        for size in 1..=80 {
            let mut hay = vec![b'.'; size];
            hay[0] = b'X';
            let mut dfa = make_dfa("[XY]").unwrap();
            let result = dfa.find(&String::from_utf8_lossy(&hay));
            assert!(result.is_some(), "size={}", size);

            hay[0] = b'.';
            hay[size - 1] = b'Y';
            let result = dfa.find(&String::from_utf8_lossy(&hay));
            assert!(result.is_some(), "size={}", size);
        }
    }

    #[test]
    fn test_rev_search_no_match_long() {
        let hay = vec![b'.'; 1024];
        let mut dfa = make_dfa("Z").unwrap();
        let result = dfa.find(&String::from_utf8_lossy(&hay));
        assert!(result.is_none());
    }

    #[test]
    fn test_rev_search_all_match() {
        let hay = vec![b'Z'; 100];
        let mut dfa = make_dfa("Z").unwrap();
        let matches = dfa.find_all(&String::from_utf8_lossy(&hay));
        assert_eq!(matches.len(), 100);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 1);
        assert_eq!(matches[99].start, 99);
        assert_eq!(matches[99].end, 100);
    }

    #[test]
    fn test_fwd_literal_at_every_offset() {
        // Literal at every possible offset
        for pos in 0..98 {
            let mut hay = vec![b'.'; 100];
            hay[pos] = b'a';
            hay[pos + 1] = b'b';
            hay[pos + 2] = b'c';
            let mut dfa = make_dfa("abc").unwrap();
            let result = dfa.find(&String::from_utf8_lossy(&hay));
            assert!(result.is_some(), "pos={}", pos);
            assert_eq!(result.unwrap().start, pos, "pos={}", pos);
        }
    }

    #[test]
    fn test_fwd_literal_adjacent_non_overlapping() {
        let mut dfa = make_dfa("abab").unwrap();
        let matches = dfa.find_all("abababababab");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 4);
        assert_eq!(matches[1].start, 4);
        assert_eq!(matches[1].end, 8);
        assert_eq!(matches[2].start, 8);
        assert_eq!(matches[2].end, 12);
    }

    #[test]
    fn test_fwd_literal_long_needle() {
        let needle16 = b"ABCDEFGHIJKLMNOP";
        let mut hay = vec![b'.'; 200];
        hay[100..116].copy_from_slice(needle16);
        let mut dfa = make_dfa("ABCDEFGHIJKLMNOP").unwrap();
        let matches = dfa.find_all(&String::from_utf8_lossy(&hay));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 100);
        assert_eq!(matches[0].end, 116);
    }

    #[test]
    fn test_fwd_literal_haystack_equals_needle() {
        let mut dfa = make_dfa("exact").unwrap();
        let m = dfa.find("exact").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn test_fwd_literal_single_byte() {
        let mut dfa = make_dfa("a").unwrap();
        assert!(dfa.find("a").is_some());
        assert!(dfa.find("b").is_none());
    }

    #[test]
    fn test_fwd_literal_near_end_boundary() {
        for size in [15, 16, 17, 31, 32, 33, 47, 48, 49, 63, 64, 65] {
            let mut hay = vec![b'.'; size];
            if size >= 3 {
                hay[size - 3] = b'x';
                hay[size - 2] = b'y';
                hay[size - 1] = b'z';
                let mut dfa = make_dfa("xyz").unwrap();
                let result = dfa.find(&String::from_utf8_lossy(&hay));
                assert!(result.is_some(), "size={}", size);
                assert_eq!(result.unwrap().start, size - 3, "size={}", size);
            }
        }
    }

    #[test]
    fn test_digit_class_boundary_sweep() {
        // Test digit class at various boundary sizes
        for size in [10, 15, 16, 17, 31, 32, 33, 48, 64, 100] {
            let mut hay = vec![b'.'; size];
            let mid = size / 2;
            hay[mid] = b'5';
            hay[mid + 1] = b'7';
            let mut dfa = make_dfa("[0-9]+").unwrap();
            let result = dfa.find(&String::from_utf8_lossy(&hay));
            assert!(result.is_some(), "size={}", size);
        }
    }

    #[test]
    fn test_digit_class_dense_matches() {
        // Dense digit matches
        let hay: Vec<u8> = (0..100).map(|i| b'0' + (i % 10)).collect();
        let mut dfa = make_dfa("[0-9]+").unwrap();
        let matches = dfa.find_all(&String::from_utf8_lossy(&hay));
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_word_class_boundary() {
        let mut dfa = make_dfa(r"[A-Z][a-z]+").unwrap();
        let result = dfa.find("Hello World Foo Bar");
        assert!(result.is_some());
    }

    #[test]
    fn test_alternation_three_way() {
        let mut dfa = make_dfa("cat|dog|fox").unwrap();
        let result = dfa.find("the cat sat on the dog and the fox ran");
        assert!(result.is_some());
    }

    #[test]
    fn test_char_class_no_match_long() {
        let hay = vec![b'.'; 200];
        let mut dfa = make_dfa("[0-9]+").unwrap();
        assert!(dfa.find(&String::from_utf8_lossy(&hay)).is_none());
    }

    #[test]
    fn test_char_class_at_position_zero() {
        let mut dfa = make_dfa("[0-9]+").unwrap();
        let m = dfa.find("123abc").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn test_char_class_at_end() {
        let mut dfa = make_dfa("[0-9]+").unwrap();
        let m = dfa.find("abc123").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_rev_char_class_at_end() {
        let mut dfa = make_dfa("[0-9]+").unwrap();
        let m = dfa.find_rev("abc123").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_rev_literal_at_end() {
        let mut dfa = make_dfa("hello").unwrap();
        let m = dfa.find_rev("say hello world").unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 9);
    }

    #[test]
    fn test_rev_literal_long_pattern() {
        let mut dfa = make_dfa("the quick brown fox").unwrap();
        let m = dfa.find_rev("the quick brown fox and the quick brown fox").unwrap();
        assert_eq!(m.start, 24);
        assert_eq!(m.end, 43);
    }

    #[test]
    fn test_rev_literal_at_boundary_32() {
        // Test reverse search at 32-byte boundary
        for offset in [0, 31, 32, 33, 63, 64, 65] {
            let mut hay = vec![b'.'; 128];
            if offset + 5 <= 128 {
                hay[offset] = b'h';
                hay[offset + 1] = b'e';
                hay[offset + 2] = b'l';
                hay[offset + 3] = b'l';
                hay[offset + 4] = b'o';
                let mut dfa = make_dfa("hello").unwrap();
                let m = dfa.find_rev(&String::from_utf8_lossy(&hay));
                assert!(m.is_some(), "offset={}", offset);
            }
        }
    }

    #[test]
    fn test_find_all_multiple_matches() {
        let mut dfa = make_dfa("the").unwrap();
        let text = "the quick brown fox jumps over the lazy dog and the cat";
        let matches = dfa.find_all(text);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[1].start, 31);
        assert_eq!(matches[2].start, 48);
    }

    #[test]
    fn test_empty_literal() {
        let mut dfa = make_dfa("").unwrap();
        // Empty pattern should match at position 0
        let m = dfa.find("hello");
        // Note: Empty patterns may not be supported in all regex engines
        assert!(m.is_none() || m.unwrap().start == 0);
    }

    #[test]
    fn test_pattern_at_text_start() {
        let mut dfa = make_dfa("hello").unwrap();
        let m = dfa.find("hello world").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn test_pattern_at_text_end() {
        let mut dfa = make_dfa("world").unwrap();
        let m = dfa.find("hello world").unwrap();
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 11);
    }

    #[test]
    fn test_pattern_not_found() {
        let mut dfa = make_dfa("xyz").unwrap();
        assert!(dfa.find("hello world").is_none());
    }

    #[test]
    fn test_single_char_pattern() {
        let mut dfa = make_dfa("x").unwrap();
        let text = "abcxdef";
        let m = dfa.find(text).unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn test_repeating_pattern() {
        let mut dfa = make_dfa("aa").unwrap();
        let text = "aaaaaa";
        let matches = dfa.find_all(text);
        // Should find overlapping or non-overlapping matches
        assert!(!matches.is_empty());
    }

    #[test]
    fn benchmark_literal_search() {
        let mut dfa = make_dfa("the quick brown fox").unwrap();
        let text = "the quick brown fox jumps over the lazy dog the quick brown fox";
        
        // Warm up
        for _ in 0..1000 {
            let _ = dfa.find(text);
            let _ = dfa.find_rev(text);
        }
        
        // Benchmark forward search
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            black_box(dfa.find(text));
        }
        let fwd_time = start.elapsed();
        
        // Benchmark reverse search
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            black_box(dfa.find_rev(text));
        }
        let rev_time = start.elapsed();
        
        println!("\n=== Literal Search Benchmark (19-byte pattern) ===");
        println!("Pattern: 'the quick brown fox' (uses Teddy for >= 4 bytes)");
        println!("Text: 'the quick brown fox jumps over the lazy dog...'");
        println!("Forward (100k): {:?} ({:.2} ns/op)", fwd_time, fwd_time.as_nanos() as f64 / 100_000.0);
        println!("Reverse (100k): {:?} ({:.2} ns/op)", rev_time, rev_time.as_nanos() as f64 / 100_000.0);
    }

    #[test]
    fn benchmark_short_literal() {
        // Short patterns (3 bytes) use memchr, not Teddy
        let mut dfa = make_dfa("abc").unwrap();
        let text = "xyz abc def abc ghi abc";
        
        // Warm up
        for _ in 0..1000 {
            let _ = dfa.find(text);
            let _ = dfa.find_rev(text);
        }
        
        // Benchmark
        let iterations = 100_000;
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(dfa.find(text));
        }
        let fwd_time = start.elapsed();
        
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(dfa.find_rev(text));
        }
        let rev_time = start.elapsed();
        
        println!("\n=== Short Literal Benchmark (3-byte pattern) ===");
        println!("Pattern: 'abc' (uses memchr, not Teddy)");
        println!("Forward (100k): {:?} ({:.2} ns/op)", fwd_time, fwd_time.as_nanos() as f64 / 100_000.0);
        println!("Reverse (100k): {:?} ({:.2} ns/op)", rev_time, rev_time.as_nanos() as f64 / 100_000.0);
    }

    #[test]
    fn benchmark_digit_class() {
        let mut dfa = make_dfa("[0-9]+").unwrap();
        let text = "Lorem ipsum dolor sit amet 12345 consectetur adipiscing elit 67890";
        
        // Warm up
        for _ in 0..1000 {
            let _ = dfa.find(text);
            let _ = dfa.find_rev(text);
        }
        
        // Benchmark forward search
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            black_box(dfa.find(text));
        }
        let fwd_time = start.elapsed();
        
        // Benchmark reverse search
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            black_box(dfa.find_rev(text));
        }
        let rev_time = start.elapsed();
        
        println!("\n=== Digit Class Benchmark [0-9]+ ===");
        println!("Pattern: '[0-9]+' (uses CharClassRanges + SIMD)");
        println!("Text: 'Lorem ipsum dolor... 12345 ... 67890'");
        println!("Forward (100k): {:?} ({:.2} ns/op)", fwd_time, fwd_time.as_nanos() as f64 / 100_000.0);
        println!("Reverse (100k): {:?} ({:.2} ns/op)", rev_time, rev_time.as_nanos() as f64 / 100_000.0);
    }

    #[test]
    fn benchmark_large_text() {
        let large_text = "Lorem ipsum dolor sit amet ".repeat(1000);
        let mut dfa_literal = make_dfa("ipsum").unwrap();
        let mut dfa_digit = make_dfa("[0-9]+").unwrap();
        let mut dfa_long = make_dfa("the quick brown fox jumps over the lazy dog").unwrap();
        
        // Warm up
        for _ in 0..100 {
            let _ = dfa_literal.find(&large_text);
            let _ = dfa_literal.find_rev(&large_text);
        }
        
        // Benchmark literal (rare byte 'x')
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            black_box(dfa_literal.find(&large_text));
        }
        let lit_fwd = start.elapsed();
        
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            black_box(dfa_literal.find_rev(&large_text));
        }
        let lit_rev = start.elapsed();
        
        // Benchmark digit class (no digits in text = worst case)
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            black_box(dfa_digit.find(&large_text));
        }
        let digit_fwd = start.elapsed();
        
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            black_box(dfa_digit.find_rev(&large_text));
        }
        let digit_rev = start.elapsed();
        
        // Benchmark long pattern (Teddy target)
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            black_box(dfa_long.find(&large_text));
        }
        let long_fwd = start.elapsed();
        
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            black_box(dfa_long.find_rev(&large_text));
        }
        let long_rev = start.elapsed();
        
        println!("\n=== Large Text (50KB) Benchmark ===");
        println!("Text: 1000x 'Lorem ipsum dolor sit amet '");
        println!("");
        println!("Literal 'ipsum' (5 bytes):");
        println!("  Forward: {:?} ({:.2} ns/op)", lit_fwd, lit_fwd.as_nanos() as f64 / 10_000.0);
        println!("  Reverse: {:?} ({:.2} ns/op)", lit_rev, lit_rev.as_nanos() as f64 / 10_000.0);
        println!("");
        println!("Digit [0-9]+ (no match, worst case):");
        println!("  Forward: {:?} ({:.2} ns/op)", digit_fwd, digit_fwd.as_nanos() as f64 / 10_000.0);
        println!("  Reverse: {:?} ({:.2} ns/op)", digit_rev, digit_rev.as_nanos() as f64 / 10_000.0);
        println!("");
        println!("Long pattern (42 bytes, Teddy target):");
        println!("  Forward: {:?} ({:.2} ns/op)", long_fwd, long_fwd.as_nanos() as f64 / 10_000.0);
        println!("  Reverse: {:?} ({:.2} ns/op)", long_rev, long_rev.as_nanos() as f64 / 10_000.0);
    }

    #[test]
    fn benchmark_rare_vs_common_byte() {
        let large_text = "the quick brown fox jumps over the lazy dog ".repeat(1000);
        
        // Pattern with common byte 'e'
        let mut dfa_common = make_dfa("the").unwrap();
        // Pattern with rare byte 'x'  
        let mut dfa_rare = make_dfa("fox").unwrap();
        
        // Warm up
        for _ in 0..100 {
            let _ = dfa_common.find(&large_text);
            let _ = dfa_rare.find(&large_text);
        }
        
        let iterations = 10_000;
        
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(dfa_common.find(&large_text));
        }
        let common_time = start.elapsed();
        
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(dfa_rare.find(&large_text));
        }
        let rare_time = start.elapsed();
        
        println!("\n=== Rare vs Common Byte Benchmark ===");
        println!("Text: 1000x 'the quick brown fox jumps over the lazy dog '");
        println!("");
        println!("Pattern 'the' (common 'e'): {:?} ({:.2} ns/op)", 
            common_time, common_time.as_nanos() as f64 / iterations as f64);
        println!("Pattern 'fox' (rare 'x'): {:?} ({:.2} ns/op)", 
            rare_time, rare_time.as_nanos() as f64 / iterations as f64);
        println!("");
        println!("Speedup from rare byte: {:.2}x", 
            common_time.as_nanos() as f64 / rare_time.as_nanos() as f64);
    }

    fn black_box<T>(x: T) -> T { x }
}
