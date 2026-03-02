#![warn(clippy::pedantic)]
#![allow(
    clippy::let_underscore_untyped,
    clippy::struct_excessive_bools,
    clippy::float_cmp,
    let_underscore_drop,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    clippy::cast_lossless,
    clippy::cast_ptr_alignment,
    clippy::ptr_as_ptr
)]

//! # Fuzzy Regex
//!
//! A fuzzy regular expression engine that combines traditional regex constructs
//! with fuzzy (approximate) string matching using Damerau-Levenshtein automata.
//!
//! ## Features
//!
//! - **Fuzzy literal matching**: Match strings with edit distance tolerance
//! - **Full regex support**: Character classes, quantifiers, groups, alternation, anchors
//! - **Per-segment fuzziness**: Control fuzziness for individual parts of a pattern
//! - **Capture groups**: Named and numbered capture groups
//! - **Similarity scoring**: Get match quality scores (0.0 - 1.0)
//!
//! ## Quick Start
//!
//! ```rust
//! use fuzzy_regex::FuzzyRegex;
//!
//! // Simple fuzzy matching - hello~2 allows up to 2 edits
//! let re = FuzzyRegex::builder("hello~2")
//!     .similarity(0.7)
//!     .build()
//!     .unwrap();
//!
//! assert!(re.is_match("hello"));  // Exact match
//! assert!(re.is_match("helo"));   // 1 deletion
//! assert!(re.is_match("helllo")); // 1 insertion
//! ```
//!
//! ## Pattern Syntax
//!
//! ### Fuzziness Markers
//!
//! - `hello~2` - Allow up to 2 edits in "hello"
//! - `hello~0` - Exact match only (no edits)
//! - `hello~{i=1,d=2,s=0}` - Detailed limits: 1 insertion, 2 deletions, 0 substitutions
//! - `(hello world)~2` - Fuzziness on entire group
//!
//! ### Standard Regex Constructs
//!
//! - `[a-z]`, `\d`, `\w`, `\s` - Character classes
//! - `*`, `+`, `?`, `{n,m}` - Quantifiers
//! - `(group)`, `(?:non-capture)` - Groups
//! - `(?<name>...)` - Named capture groups
//! - `a|b` - Alternation
//! - `^`, `$` - Anchors
//! - `(?=...)`, `(?!...)` - Lookahead
//!
//! ## Examples
//!
//! ### Fuzzy Search
//!
//! ```rust
//! use fuzzy_regex::FuzzyRegexBuilder;
//!
//! let re = FuzzyRegexBuilder::new(r"color~2|colour~2")
//!     .similarity(0.8)
//!     .build()
//!     .unwrap();
//!
//! // Matches various spellings
//! assert!(re.is_match("color"));
//! assert!(re.is_match("colour"));
//! assert!(re.is_match("colur")); // With typo
//! ```
//!
//! ### Capture Groups with Fuzzy Matching
//!
//! ```rust
//! use fuzzy_regex::FuzzyRegex;
//!
//! let re = FuzzyRegex::new(r"(?<word>\w+)").unwrap();
//! let caps = re.captures("hello").unwrap();
//! assert_eq!(caps.name("word").unwrap().as_str(), "hello");
//! ```
//!
//! ### Replace with Fuzzy Matching
//!
//! ```rust
//! use fuzzy_regex::FuzzyRegexBuilder;
//!
//! // Match "recieve" (common misspelling) and replace with correct spelling
//! // Note: fuzzy matching allows edits including insertions/deletions at boundaries,
//! // so we use exact match here (~0) to replace just the misspelled word
//! let re = FuzzyRegexBuilder::new(r"recieve~0")
//!     .build()
//!     .unwrap();
//!
//! let result = re.replace("Did you recieve the package?", "receive");
//! assert_eq!(result, "Did you receive the package?");
//! ```

// Re-export mimalloc for consumers who want to use it as global allocator
#[cfg(feature = "mimalloc")]
pub use mimalloc::MiMalloc;

pub mod api;
pub mod compat;
/// Compiler for converting parsed regex AST into executable IR.
pub mod compiler;
/// Core matching engines including NFA simulation and Bitap algorithm.
pub mod engine;
pub mod error;
/// Intermediate representation for compiled regex patterns.
pub mod ir;
/// Regex pattern parser and lexer.
pub mod parser;
pub mod types;

// Re-export main types at crate root
pub use api::{
    ByteMatches, CaptureMatches, Captures, FeedMatches, FuzzyRegex, FuzzyRegexBuilder, Handler,
    HandlerMap, HandlerResult, Match, MatchFlags, Matches, ReaderMatches, RegexConfig, Split,
    StreamingMatch, StreamingMatcher,
};
pub use engine::EditCounts;
pub use error::{Error, Result};
pub use types::{FuzzyLimits, FuzzyPenalties};
