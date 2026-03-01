//! Public API module.
//!
//! This module exports the main types for using fuzzy regex:
//! - `FuzzyRegex`: The compiled regex type
//! - `FuzzyRegexBuilder`: Builder for customized regex construction
//! - `Match`, `Captures`: Match result types
//! - `StreamingMatcher`, `StreamingMatch`: Streaming API types

pub mod builder;
pub mod match_result;
pub mod regex;
pub mod streaming;

pub use builder::{FuzzyRegexBuilder, MatchFlags, RegexConfig};
pub use match_result::{CaptureMatches, Captures, Match, Matches, Split};
pub use regex::FuzzyRegex;
pub use streaming::{ByteMatches, FeedMatches, ReaderMatches, StreamingMatch, StreamingMatcher};
