//! Builder for `FuzzyRegex`.

use crate::engine::FxHashMap;
use crate::types::{FuzzyLimits, FuzzyPenalties};
use std::sync::Arc;

use super::regex::FuzzyRegex;
use crate::error::Result;

/// Result from a custom handler.
///
/// - `MatchOverride(n, text)` - handler matched, consume n bytes, override capture with text
/// - `NoMatch` - handler did not match
#[derive(Debug, Clone)]
pub enum HandlerResult {
    /// Handler matched: consumes n bytes and overrides the captured text.
    /// The capture for the matched region will be set to `text` instead of
    /// the actual input slice.
    MatchOverride(usize, String),
    /// Handler did not match.
    NoMatch,
}

/// A custom handler function that can be called from within a regex pattern.
///
/// The handler is invoked when the regex engine reaches a `(?call:name)` position.
/// It receives the text being matched and the current position, and returns a `HandlerResult`.
pub type Handler = dyn Fn(&str, usize) -> HandlerResult + Send + Sync;

/// A map of handler names to handler functions.
pub type HandlerMap = FxHashMap<std::sync::Arc<str>, std::sync::Arc<Handler>>;

/// Builder for constructing a `FuzzyRegex` with custom configuration.
#[derive(Clone)]
pub struct FuzzyRegexBuilder {
    pattern: String,
    config: RegexConfig,
}

/// Flags controlling match behavior.
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct MatchFlags {
    /// `BESTMATCH` flag - find best match instead of first.
    pub best_match: bool,
    /// `ENHANCEMATCH` flag - improve match quality.
    pub enhance_match: bool,
    /// `POSIX` flag - find longest match at leftmost position.
    pub posix: bool,
    /// `(?x)` - Verbose mode (ignore whitespace, allow comments).
    pub verbose: bool,
    /// `(?s)` - Dot-all mode (`.` matches newlines).
    pub dot_all: bool,
    /// `(?m)` - Multi-line mode (`^`/`$` match at line boundaries).
    pub multi_line: bool,
    /// `(?U)` - Ungreedy mode (invert greediness of quantifiers).
    /// When set, `*`, `+`, `?` become non-greedy by default, and `*?`, `+?`, `??` become greedy.
    pub ungreedy: bool,
    /// `(?g)` - Global mode (find all matches).
    /// When false (default), stops at first valid match (faster).
    /// When true, searches for all matches.
    pub global: bool,
    /// `(?u)` - Unicode mode (enable Unicode character classes).
    /// When set, \w, \d, \s match Unicode characters instead of ASCII only.
    pub unicode: bool,
}

/// Configuration for regex matching.
#[allow(clippy::struct_excessive_bools)]
pub struct RegexConfig {
    /// Case-insensitive matching.
    pub case_insensitive: bool,
    /// Verbose mode - ignore whitespace and allow `#` comments in pattern.
    pub verbose: bool,
    /// Dot-all mode - `.` matches newlines.
    pub dot_all: bool,
    /// Multi-line mode - `^` and `$` match at line boundaries.
    pub multi_line: bool,
    /// Ungreedy mode - invert default greediness of quantifiers.
    pub ungreedy: bool,
    /// Default similarity threshold.
    pub similarity_threshold: f32,
    /// Default number of edits allowed.
    pub default_edits: u8,
    /// Default fuzzy limits.
    pub default_limits: Option<FuzzyLimits>,
    /// Edit penalties.
    pub penalties: Option<FuzzyPenalties>,
    /// Maximum threads for NFA simulation (beam width).
    pub max_threads: usize,
    /// Match behavior flags.
    pub match_flags: MatchFlags,
    /// Partial matching - allow matches that reach end of text.
    pub partial: bool,
    /// Default timeout for matching operations.
    pub timeout: Option<std::time::Duration>,
    /// Greedy first-match mode - return first match found (faster).
    /// Similar to mrab-regex behavior - searches position by position,
    /// returning on first match instead of searching for best match.
    pub greedy_first: bool,
    /// Custom handlers registered for this regex.
    pub handlers: HandlerMap,
}

impl Clone for RegexConfig {
    fn clone(&self) -> Self {
        RegexConfig {
            case_insensitive: self.case_insensitive,
            verbose: self.verbose,
            dot_all: self.dot_all,
            multi_line: self.multi_line,
            ungreedy: self.ungreedy,
            similarity_threshold: self.similarity_threshold,
            default_edits: self.default_edits,
            default_limits: self.default_limits.clone(),
            penalties: self.penalties.clone(),
            max_threads: self.max_threads,
            match_flags: self.match_flags,
            partial: self.partial,
            timeout: self.timeout,
            greedy_first: self.greedy_first,
            handlers: self.handlers.clone(), // Note: handlers can't really be cloned properly, this is a shallow copy
        }
    }
}

impl Default for RegexConfig {
    fn default() -> Self {
        RegexConfig {
            case_insensitive: false,
            verbose: false,
            dot_all: false,
            multi_line: false,
            ungreedy: false,
            // Default to 0.0 so that edit-limited patterns like {e<=N} accept all valid matches.
            // Users can set a higher threshold with .similarity() to filter by quality.
            // This matches mrab-regex behavior where only edit limits matter, not similarity.
            similarity_threshold: 0.0,
            default_edits: 0,
            default_limits: None,
            penalties: None,
            max_threads: 1000,
            match_flags: MatchFlags::default(),
            partial: false,
            timeout: None,
            greedy_first: false,
            handlers: FxHashMap::default(),
        }
    }
}

impl FuzzyRegexBuilder {
    /// Create a new builder with the given pattern.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        FuzzyRegexBuilder {
            pattern: pattern.to_string(),
            config: RegexConfig::default(),
        }
    }

    /// Set case-insensitive matching.
    #[must_use]
    pub fn case_insensitive(mut self, yes: bool) -> Self {
        self.config.case_insensitive = yes;
        self
    }

    /// Enable verbose mode (ignore whitespace and allow `#` comments).
    ///
    /// In verbose mode:
    /// - Whitespace is ignored (use `\s` or `[ ]` for literal space)
    /// - `#` starts a comment that extends to end of line
    ///
    /// This allows formatting patterns for readability:
    /// ```text
    /// (?x)
    /// [A-Z][a-z]+     # First name
    /// \s+             # Whitespace
    /// [A-Z][a-z]+     # Last name
    /// ```
    #[must_use]
    pub fn verbose(mut self, yes: bool) -> Self {
        self.config.verbose = yes;
        self
    }

    /// Enable dot-all mode (`.` matches newlines).
    ///
    /// By default, `.` matches any character except newlines.
    /// When enabled, `.` matches any character including `\n`.
    #[must_use]
    pub fn dot_all(mut self, yes: bool) -> Self {
        self.config.dot_all = yes;
        self
    }

    /// Enable multi-line mode (`^` and `$` match at line boundaries).
    ///
    /// By default, `^` matches only at the start of text and `$` only at the end.
    /// When enabled:
    /// - `^` also matches after each newline
    /// - `$` also matches before each newline
    #[must_use]
    pub fn multi_line(mut self, yes: bool) -> Self {
        self.config.multi_line = yes;
        self
    }

    /// Enable ungreedy mode (invert default greediness of quantifiers).
    ///
    /// When enabled:
    /// - `*`, `+`, `?` become non-greedy (match as little as possible)
    /// - `*?`, `+?`, `??` become greedy (match as much as possible)
    ///
    /// This is equivalent to the `(?U)` inline flag.
    #[must_use]
    pub fn ungreedy(mut self, yes: bool) -> Self {
        self.config.ungreedy = yes;
        self
    }

    /// Enable global mode (find all matches).
    ///
    /// When enabled, use `find_iter()` to get all matches.
    /// When disabled (default), stops at first valid match (faster).
    /// This is equivalent to the `(?g)` inline flag.
    #[must_use]
    pub fn global(mut self, yes: bool) -> Self {
        self.config.match_flags.global = yes;
        self
    }

    /// Enable greedy first-match mode (similar to mrab-regex behavior).
    ///
    /// When enabled, searches position by position and returns the first match found.
    /// This is faster than the default best-match behavior when matches exist early
    /// in the text, but may not find the optimal match.
    ///
    /// Default behavior (`greedy_first=false`): searches all positions to find best match.
    /// Greedy first (`greedy_first=true`): stops at first match found.
    #[must_use]
    pub fn greedy_first(mut self, yes: bool) -> Self {
        self.config.greedy_first = yes;
        self
    }

    /// Enable Unicode mode for character classes.
    ///
    /// When enabled:
    /// - `\w` matches Unicode word characters (not just ASCII `[a-zA-Z0-9_]`)
    /// - `\d` matches Unicode digits
    /// - `\s` matches Unicode whitespace
    ///
    /// This is equivalent to the `(?u)` inline flag.
    #[must_use]
    pub fn unicode(mut self, yes: bool) -> Self {
        self.config.match_flags.unicode = yes;
        self
    }

    /// Set the similarity threshold (0.0 - 1.0).
    ///
    /// Matches with similarity below this threshold are rejected.
    #[must_use]
    pub fn similarity(mut self, threshold: f32) -> Self {
        self.config.similarity_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the default number of edits allowed for fuzzy matching.
    ///
    /// This applies to literals without explicit fuzziness markers.
    #[must_use]
    pub fn edits(mut self, count: u8) -> Self {
        self.config.default_edits = count;
        self
    }

    /// Set detailed fuzzy limits for default matching.
    #[must_use]
    pub fn fuzzy(mut self, limits: FuzzyLimits) -> Self {
        self.config.default_limits = Some(limits);
        self
    }

    /// Set edit operation penalties.
    #[must_use]
    pub fn penalties(mut self, penalties: FuzzyPenalties) -> Self {
        self.config.penalties = Some(penalties);
        self
    }

    /// Set the maximum number of threads for NFA simulation.
    ///
    /// Higher values allow more complex patterns but use more memory.
    #[must_use]
    pub fn max_threads(mut self, max: usize) -> Self {
        self.config.max_threads = max;
        self
    }

    /// Enable partial matching.
    ///
    /// When enabled, matches that reach the end of the text are considered
    /// successful even if they haven't completed. The `Match::partial()` method
    /// can be used to check if a match is partial.
    ///
    /// This is useful for streaming input or when the text may be truncated.
    #[must_use]
    pub fn partial(mut self, yes: bool) -> Self {
        self.config.partial = yes;
        self
    }

    /// Set a timeout for matching operations.
    ///
    /// If a match operation takes longer than the timeout, it will be cancelled.
    /// The default is no timeout.
    ///
    /// Note: Timeout is checked at certain checkpoints during matching, so it's
    /// not precise. The actual time may exceed the timeout slightly.
    #[must_use]
    pub fn timeout(mut self, duration: std::time::Duration) -> Self {
        self.config.timeout = Some(duration);
        self
    }

    /// Register a custom handler that can be invoked from within the regex pattern.
    ///
    /// Handlers are called when the regex engine reaches a `(?call:name)` position.
    /// The handler receives the text being matched and the current position, and returns:
    /// - `HandlerResult::MatchOverride(n, text)` if the handler matches:
    ///   - `n` is the number of characters consumed from the input
    ///   - `text` is the override text to use for captures instead of the actual input slice
    /// - `HandlerResult::NoMatch` if the handler does not match
    ///
    /// # Example
    ///
    /// ```rust
    /// use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};
    ///
    /// let re = FuzzyRegexBuilder::new(r#"(?call:unescape)"#)
    ///     .handler("unescape", |text, pos| {
    ///         // Simple handler that matches a backslash-escaped character
    ///         if pos + 1 < text.len() && text.as_bytes()[pos] == b'\\' {
    ///             let escaped = &text[pos..pos+2];
    ///             HandlerResult::MatchOverride(2, escaped.to_string())
    ///         } else {
    ///             HandlerResult::NoMatch
    ///         }
    ///     })
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub fn handler(
        mut self,
        name: &str,
        handler: impl Fn(&str, usize) -> HandlerResult + Send + Sync + 'static,
    ) -> Self {
        self.config.handlers.insert(name.into(), Arc::new(handler));
        self
    }

    /// Build the `FuzzyRegex`.
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is invalid or cannot be compiled.
    pub fn build(self) -> Result<FuzzyRegex> {
        FuzzyRegex::compile(self.pattern, self.config)
    }
}
