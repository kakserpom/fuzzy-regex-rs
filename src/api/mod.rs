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
#[cfg(feature = "word-list-ac")]
pub(crate) mod word_list_ac;

pub use builder::{
    FuzzyRegexBuilder, Handler, HandlerMap, HandlerResult, MatchEndPolicy, MatchFlags, RegexConfig,
};
pub use match_result::{CaptureMatches, Captures, Match, Matches, Replacer, Split};
pub use regex::FuzzyRegex;
pub use streaming::{ByteMatches, FeedMatches, ReaderMatches, StreamingMatch, StreamingMatcher};
