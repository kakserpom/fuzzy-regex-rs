//! Matching engine module.
//!
//! This module contains the core NFA simulation engine,
//! DFA for fast exact matching, and the bridge to fuzzy matching.

// Module-level allows for implementation-specific patterns

pub mod backtrack;
pub mod bitap;
pub mod captures;
pub mod damlev;
pub mod dfa;
pub mod fuzzy_bridge;
pub mod fuzzy_nfa;
pub mod guard_nfa;
pub mod hash;
pub mod matcher;
pub mod myers;
pub mod prefilter;
pub mod simd_class;

pub use backtrack::BacktrackMatcher;
pub use bitap::{MultiBitapMatcher, MultiPatternCandidate, MultiPatternMatch};
pub use captures::{CaptureState, CaptureStateBuilder};
pub use dfa::{Dfa, DfaMatch};
pub use fuzzy_bridge::{CachedMatches, FuzzyBridge, FuzzyMatchResult};
pub use guard_nfa::GuardNfa;
pub use hash::{FxHashMap, FxHashSet};
pub use matcher::{EditCounts, MatchResult, Matcher, MatcherConfig};
pub use prefilter::Prefilter;
pub use simd_class::{AsciiClassBitmap, CompiledCharClass};
