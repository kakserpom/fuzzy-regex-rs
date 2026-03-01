//! Error types for the fuzzy regex engine.

use std::fmt;
use thiserror::Error;

/// The main error type for fuzzy regex operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Error during pattern parsing.
    #[error("parse error at position {position}: {message}")]
    Parse {
        /// Byte position in the pattern where the error occurred.
        position: usize,
        /// Description of the parse error.
        message: String,
    },

    /// Invalid escape sequence.
    #[error("invalid escape sequence '\\{char}' at position {position}")]
    InvalidEscape {
        /// The invalid escape character.
        char: char,
        /// Byte position in the pattern where the invalid escape occurred.
        position: usize,
    },

    /// Unclosed group or bracket.
    #[error("unclosed {kind} starting at position {position}")]
    Unclosed {
        /// The kind of unclosed delimiter (e.g., "group", "bracket").
        kind: &'static str,
        /// Byte position in the pattern where the unclosed delimiter starts.
        position: usize,
    },

    /// Invalid quantifier.
    #[error("invalid quantifier at position {position}: {message}")]
    InvalidQuantifier {
        /// Byte position in the pattern where the invalid quantifier occurred.
        position: usize,
        /// Description of why the quantifier is invalid.
        message: String,
    },

    /// Invalid character class.
    #[error("invalid character class at position {position}: {message}")]
    InvalidCharClass {
        /// Byte position in the pattern where the invalid character class occurred.
        position: usize,
        /// Description of why the character class is invalid.
        message: String,
    },

    /// Invalid fuzziness specification.
    #[error("invalid fuzziness specification at position {position}: {message}")]
    InvalidFuzziness {
        /// Byte position in the pattern where the invalid fuzziness specification occurred.
        position: usize,
        /// Description of why the fuzziness specification is invalid.
        message: String,
    },

    /// Invalid backreference.
    #[error("invalid backreference \\{group} at position {position}: {message}")]
    InvalidBackreference {
        /// The backreference group number that is invalid.
        group: usize,
        /// Byte position in the pattern where the invalid backreference occurred.
        position: usize,
        /// Description of why the backreference is invalid.
        message: String,
    },

    /// Empty pattern where one was required.
    #[error("empty pattern not allowed")]
    EmptyPattern,

    /// Pattern too complex (e.g., too many states).
    #[error("pattern too complex: {message}")]
    TooComplex {
        /// Description of why the pattern is too complex.
        message: String,
    },

    /// Match operation timed out.
    #[error("match timed out after {duration:?}")]
    Timeout {
        /// The timeout duration that was exceeded.
        duration: std::time::Duration,
    },
}

impl Error {
    /// Create a parse error at a given position.
    #[must_use]
    pub fn parse(position: usize, message: impl Into<String>) -> Self {
        Error::Parse {
            position,
            message: message.into(),
        }
    }

    /// Create an invalid escape error.
    #[must_use]
    pub fn invalid_escape(char: char, position: usize) -> Self {
        Error::InvalidEscape { char, position }
    }

    /// Create an unclosed delimiter error.
    #[must_use]
    pub fn unclosed(kind: &'static str, position: usize) -> Self {
        Error::Unclosed { kind, position }
    }

    /// Create an invalid quantifier error.
    #[must_use]
    pub fn invalid_quantifier(position: usize, message: impl Into<String>) -> Self {
        Error::InvalidQuantifier {
            position,
            message: message.into(),
        }
    }

    /// Create an invalid character class error.
    #[must_use]
    pub fn invalid_char_class(position: usize, message: impl Into<String>) -> Self {
        Error::InvalidCharClass {
            position,
            message: message.into(),
        }
    }

    /// Create an invalid fuzziness error.
    #[must_use]
    pub fn invalid_fuzziness(position: usize, message: impl Into<String>) -> Self {
        Error::InvalidFuzziness {
            position,
            message: message.into(),
        }
    }

    /// Create an invalid backreference error.
    #[must_use]
    pub fn invalid_backreference(
        group: usize,
        position: usize,
        message: impl Into<String>,
    ) -> Self {
        Error::InvalidBackreference {
            group,
            position,
            message: message.into(),
        }
    }
}

/// A specialized Result type for fuzzy regex operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Span in the input pattern for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Start position (byte offset, inclusive).
    pub start: usize,
    /// End position (byte offset, exclusive).
    pub end: usize,
}

impl Span {
    /// Create a span from start to end positions.
    #[must_use]
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// Create a single-character span at the given position.
    #[must_use]
    pub fn at(position: usize) -> Self {
        Span {
            start: position,
            end: position + 1,
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end || self.start + 1 == self.end {
            write!(f, "position {}", self.start)
        } else {
            write!(f, "positions {}-{}", self.start, self.end)
        }
    }
}
