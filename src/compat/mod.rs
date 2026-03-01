//! Compatibility layer for fuzzy-aho-corasick API.
//!
//! This module provides a drop-in replacement API for `fuzzy-aho-corasick`,
//! allowing easy migration to `fuzzy-regex` with improved performance.
//!
//! # Example
//!
//! ```rust
//! use fuzzy_regex::compat::fac::FuzzyAhoCorasickBuilder;
//! use fuzzy_regex::types::FuzzyLimits;
//!
//! let engine = FuzzyAhoCorasickBuilder::new()
//!     .fuzzy(FuzzyLimits::new().edits(1))
//!     .case_insensitive(true)
//!     .build(["hello", "world"]);
//!
//! let matches = engine.search("helo wrld", 0.8);
//! assert!(!matches.is_empty());
//! ```

pub mod fac;
mod matches;
mod pattern;
#[cfg(test)]
mod tests;

// Re-export types from submodules
pub use matches::{FuzzyMatch, FuzzyMatches, Segment, UnmatchedSegment};
pub use pattern::Pattern;

// Re-export FuzzyLimits and FuzzyPenalties from main types
pub use crate::types::{FuzzyLimits, FuzzyPenalties, NumEdits};
