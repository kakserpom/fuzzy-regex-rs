//! Abstract Syntax Tree definitions for fuzzy regex patterns.

#![allow(clippy::match_same_arms, clippy::too_many_lines)]
// Note: enum_clone_variant is not a valid lint

use crate::types::FuzzyLimits;

/// Matching flags that can be set in the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchFlags {
    /// BESTMATCH flag (`(?b)`) - search for the best match instead of the first.
    pub best_match: bool,
    /// ENHANCEMATCH flag (`(?e)`) - improve the fit of fuzzy matches.
    pub enhance_match: bool,
    /// POSIX leftmost-longest flag (`(?p)`) - find longest match at leftmost position.
    pub posix: bool,
    /// Verbose flag (`(?x)`) - ignore whitespace and allow comments.
    pub verbose: bool,
    /// Dot-all flag (`(?s)`) - `.` matches newlines.
    pub dot_all: bool,
    /// Multi-line flag (`(?m)`) - `^`/`$` match at line boundaries.
    pub multi_line: bool,
    /// Ungreedy flag (`(?U)`) - invert default greediness of quantifiers.
    pub ungreedy: bool,
    /// Case-insensitive flag (`(?i)`) - match case-insensitively.
    pub case_insensitive: bool,
    /// Global flag (`(?g)`) - find all matches, not just the first.
    pub global: bool,
    /// Unicode flag (`(?u)`) - enable Unicode character classes.
    pub unicode: bool,
}

impl MatchFlags {
    /// Create a new empty flags set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the BESTMATCH flag.
    #[must_use]
    pub fn with_best_match(mut self) -> Self {
        self.best_match = true;
        self
    }

    /// Set the ENHANCEMATCH flag.
    #[must_use]
    pub fn with_enhance_match(mut self) -> Self {
        self.enhance_match = true;
        self
    }

    /// Set the POSIX flag.
    #[must_use]
    pub fn with_posix(mut self) -> Self {
        self.posix = true;
        self
    }
}

/// Fuzziness specification for a literal segment.
///
/// Supports two syntax styles:
/// 1. Simple: `hello~2` (allows 2 total edits)
/// 2. mrab-style: `(?:hello){i<=1,d<=2}` (max 1 insertion, 2 deletions)
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Fuzziness {
    /// Simple edit count: `hello~2` allows 2 total edits.
    Edits(u8),
    /// Detailed limits: `hello~{i=1,d=0,s=2}` or mrab-style `{i<=1,d<=2}`.
    Detailed(FuzzyLimits),
    /// mrab-style specification with optional cost constraints.
    /// `{i<=1,s<=2,2i+2d+1s<=4}`
    MrabStyle(MrabFuzziness),
    /// Inherit from global/parent settings.
    #[default]
    Inherited,
    /// Exact match only (no fuzzy matching): `hello~0`.
    Exact,
}

/// mrab-regex style fuzziness specification.
///
/// Supports:
/// - Error type limits: `{i<=1,d<=2,s<=3,t<=1}`
/// - Total error limits: `{e<=5}`
/// - Cost constraints: `{2i+2d+1s+1t<=4}` or `{c<=4}` (equal cost)
/// - Ranges: `{1<=e<=3}`
/// - Exclusive bounds: `{i<3}` (fewer than 3)
/// - Character class restrictions: `{s<=2:[a-z]}` (restricts edit characters to class)
/// - Unlimited errors: `{e}` (any number of errors allowed)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MrabFuzziness {
    /// Maximum insertions allowed (None = not specified).
    pub max_insertions: Option<u8>,
    /// Maximum deletions allowed.
    pub max_deletions: Option<u8>,
    /// Maximum substitutions allowed.
    pub max_substitutions: Option<u8>,
    /// Maximum transpositions allowed.
    pub max_transpositions: Option<u8>,
    /// Maximum total errors allowed.
    pub max_errors: Option<u8>,
    /// Minimum total errors required.
    pub min_errors: Option<u8>,
    /// Whether insertions are unlimited (e.g., `{i}` without a value).
    pub unlimited_insertions: bool,
    /// Whether deletions are unlimited.
    pub unlimited_deletions: bool,
    /// Whether substitutions are unlimited.
    pub unlimited_substitutions: bool,
    /// Whether transpositions are unlimited.
    pub unlimited_transpositions: bool,
    /// Whether total errors are unlimited (e.g., `{e}` without a value).
    pub unlimited_errors: bool,
    /// Cost for insertions (for cost-based constraints).
    pub insertion_cost: Option<u8>,
    /// Cost for deletions.
    pub deletion_cost: Option<u8>,
    /// Cost for substitutions.
    pub substitution_cost: Option<u8>,
    /// Cost for transpositions.
    pub transposition_cost: Option<u8>,
    /// Maximum total cost.
    pub max_cost: Option<u8>,
    /// Character class restriction for substitutions (e.g., `[a-z]`).
    /// Substituted characters must be in this class.
    pub substitution_chars: Option<CharClass>,
    /// Character class restriction for insertions.
    /// Inserted characters must be in this class.
    pub insertion_chars: Option<CharClass>,
    /// Character class restriction for deletions.
    /// Note: Deletions don't introduce new characters, so this is less meaningful.
    pub deletion_chars: Option<CharClass>,
}

impl MrabFuzziness {
    /// Create a new empty mrab-style fuzziness.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum insertions.
    #[must_use]
    pub fn insertions(mut self, max: u8) -> Self {
        self.max_insertions = Some(max);
        self
    }

    /// Set maximum deletions.
    #[must_use]
    pub fn deletions(mut self, max: u8) -> Self {
        self.max_deletions = Some(max);
        self
    }

    /// Set maximum substitutions.
    #[must_use]
    pub fn substitutions(mut self, max: u8) -> Self {
        self.max_substitutions = Some(max);
        self
    }

    /// Set maximum total errors.
    #[must_use]
    pub fn errors(mut self, max: u8) -> Self {
        self.max_errors = Some(max);
        self
    }

    /// Set error range.
    #[must_use]
    pub fn error_range(mut self, min: u8, max: u8) -> Self {
        self.min_errors = Some(min);
        self.max_errors = Some(max);
        self
    }

    /// Convert to `FuzzyLimits`.
    #[must_use]
    pub fn to_limits(&self) -> FuzzyLimits {
        // Use 255 as "unlimited" since that's the max value for u8
        const UNLIMITED: u8 = 255;

        let mut limits = FuzzyLimits::new();

        // mrab semantics: when individual operation limits are specified WITHOUT
        // an explicit total budget, every UNSPECIFIED operation defaults to 0 --
        // only the named operations are permitted. E.g. `{i<=2}` means i<=2,
        // d=0, s=0 (total 2), so text lacking the pattern's characters cannot
        // match by silently substituting/deleting.
        //
        // When an explicit total (`e<=`) or a cost constraint IS given, the
        // total budget governs the unspecified operations instead (leave them
        // unset), preserving specs like `{t<=1,e<=2}` where the e budget is the
        // intended bound for substitutions/insertions/deletions.
        let has_total =
            self.max_errors.is_some() || self.unlimited_errors || self.max_cost.is_some();
        let any_individual = !has_total
            && (self.max_insertions.is_some()
                || self.unlimited_insertions
                || self.max_deletions.is_some()
                || self.unlimited_deletions
                || self.max_substitutions.is_some()
                || self.unlimited_substitutions
                || self.max_transpositions.is_some()
                || self.unlimited_transpositions);

        // Handle insertions
        if let Some(i) = self.max_insertions {
            limits = limits.insertions(i);
        } else if self.unlimited_insertions {
            limits = limits.insertions(UNLIMITED);
        } else if any_individual {
            limits = limits.insertions(0);
        }

        // Handle deletions
        if let Some(d) = self.max_deletions {
            limits = limits.deletions(d);
        } else if self.unlimited_deletions {
            limits = limits.deletions(UNLIMITED);
        } else if any_individual {
            limits = limits.deletions(0);
        }

        // Handle substitutions
        if let Some(s) = self.max_substitutions {
            limits = limits.substitutions(s);
        } else if self.unlimited_substitutions {
            limits = limits.substitutions(UNLIMITED);
        } else if any_individual {
            limits = limits.substitutions(0);
        }

        // Handle transpositions
        if let Some(t) = self.max_transpositions {
            limits = limits.swaps(t);
        } else if self.unlimited_transpositions {
            limits = limits.swaps(UNLIMITED);
        } else if any_individual {
            limits = limits.swaps(0);
        }

        // Handle total errors
        if let Some(e) = self.max_errors {
            limits = limits.edits(e);
        } else if self.unlimited_errors {
            limits = limits.edits(UNLIMITED);
        } else if let Some(max_cost) = self.max_cost {
            // When cost constraint is used without explicit error limit,
            // infer max_edits from cost constraint. Use max_cost divided by
            // minimum operation cost (at least 1).
            let min_cost = [
                self.insertion_cost.unwrap_or(1),
                self.deletion_cost.unwrap_or(1),
                self.substitution_cost.unwrap_or(1),
                self.transposition_cost.unwrap_or(1),
            ]
            .into_iter()
            .filter(|&c| c > 0)
            .min()
            .unwrap_or(1);
            // max_cost is stored as N+1 for <=N, so subtract 1 to get actual limit
            let actual_max_cost = max_cost.saturating_sub(1);
            let inferred_max_edits = actual_max_cost / min_cost;
            limits = limits.edits(inferred_max_edits);
        }

        limits
    }

    /// Check if this fuzziness specification has any unlimited flags set.
    #[must_use]
    pub fn has_unlimited(&self) -> bool {
        self.unlimited_insertions
            || self.unlimited_deletions
            || self.unlimited_substitutions
            || self.unlimited_transpositions
            || self.unlimited_errors
    }
}

impl Fuzziness {
    /// Convert to `FuzzyLimits`, using default if inherited.
    #[must_use]
    pub fn to_limits(&self, default_edits: u8) -> Option<FuzzyLimits> {
        match self {
            Fuzziness::Exact => Some(FuzzyLimits::new().edits(0)),
            Fuzziness::Edits(n) => Some(FuzzyLimits::new().edits(*n)),
            Fuzziness::Detailed(limits) => Some(limits.clone()),
            Fuzziness::MrabStyle(mrab) => Some(mrab.to_limits()),
            Fuzziness::Inherited => {
                if default_edits > 0 {
                    Some(FuzzyLimits::new().edits(default_edits))
                } else {
                    None
                }
            }
        }
    }

    /// Get the minimum edits required (for exclusive lower bounds like `{0<e<5}`).
    #[must_use]
    pub fn min_edits(&self) -> Option<u8> {
        match self {
            Fuzziness::MrabStyle(mrab) => mrab.min_errors,
            _ => None,
        }
    }
}

/// AST node representing a parsed regex pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    /// Empty pattern.
    Empty,

    /// Literal string with optional fuzziness: `hello`, `hello~2`.
    Literal {
        /// The literal text to match.
        text: String,
        /// Fuzziness specification for approximate matching.
        fuzziness: Fuzziness,
    },

    /// Single character (from escape or plain char outside literals).
    Char(char),

    /// Character class: `[a-z]`, `[^abc]`, `\d`, `\w`, `\s`, `.`
    CharClass(CharClass),

    /// Concatenation of patterns.
    Concat(Vec<Ast>),

    /// Alternation: `a|b|c`.
    Alternation(Vec<Ast>),

    /// Quantified expression: `a*`, `a+`, `a?`, `a{n,m}`.
    Quantified {
        /// The expression being quantified.
        expr: Box<Ast>,
        /// The quantifier specifying repetition bounds.
        quantifier: Quantifier,
        /// Whether the quantifier is greedy (matches as much as possible).
        greedy: bool,
    },

    /// Capture group: `(expr)`.
    Group {
        /// The capture group index (1-based for user-facing, 0-based internally).
        index: usize,
        /// Optional name for named capture groups like `(?P<name>...)`.
        name: Option<String>,
        /// The expression contained in the group.
        expr: Box<Ast>,
    },

    /// Non-capturing group: `(?:expr)`.
    NonCapturingGroup {
        /// The expression contained in the group.
        expr: Box<Ast>,
        /// Fuzziness specification applied to this group.
        fuzziness: Fuzziness,
    },

    /// Anchor: `^`, `$`.
    Anchor(Anchor),

    /// Lookahead: `(?=...)`, `(?!...)`.
    Lookahead {
        /// True for positive lookahead `(?=...)`, false for negative `(?!...)`.
        positive: bool,
        /// The expression to match in the lookahead.
        expr: Box<Ast>,
    },

    /// Lookbehind: `(?<=...)`, `(?<!...)`.
    Lookbehind {
        /// True for positive lookbehind `(?<=...)`, false for negative `(?<!...)`.
        positive: bool,
        /// The expression to match in the lookbehind.
        expr: Box<Ast>,
    },

    /// Backreference: `\1`, `\2`, optionally with fuzziness `\1{e<=1}`.
    Backreference {
        /// The capture group number being referenced.
        group: usize,
        /// Fuzziness specification for approximate backreference matching.
        fuzziness: Fuzziness,
    },

    /// Named list reference: `\L<name>`.
    NamedList {
        /// The name of the list.
        name: String,
    },

    /// Reset match start: `\K`
    /// Resets the starting point of the match. Everything before \K is matched
    /// but not included in the final match result.
    ResetMatchStart,

    /// Atomic group: `(?>...)`
    /// Once the group matches, backtracking is disabled within the group.
    AtomicGroup {
        /// The expression contained in the atomic group.
        expr: Box<Ast>,
    },

    /// Recursive entire pattern: `(?R)`
    /// Recursively matches the entire pattern.
    RecursivePattern,

    /// Recursive numbered group: `(?1)`, `(?2)`, etc.
    /// Recursively matches a specific capture group.
    RecursiveGroup {
        /// The capture group number to recurse into.
        group: usize,
    },

    /// Recursive named group: `(?&name)` or `(?P>name)`
    /// Recursively matches a named capture group.
    RecursiveNamedGroup {
        /// The name of the capture group to recurse into.
        name: String,
    },

    /// Custom handler invocation: `(?call:name)`
    /// Calls a custom handler function at this point in the match.
    Handler {
        /// The name of the handler to invoke.
        name: String,
    },
}

impl Ast {
    /// Create a literal AST node with inherited fuzziness.
    pub fn literal(text: impl Into<String>) -> Self {
        Ast::Literal {
            text: text.into(),
            fuzziness: Fuzziness::Inherited,
        }
    }

    /// Create a literal AST node with specific fuzziness.
    pub fn literal_fuzzy(text: impl Into<String>, fuzziness: Fuzziness) -> Self {
        Ast::Literal {
            text: text.into(),
            fuzziness,
        }
    }

    /// Create a quantified AST node.
    #[must_use]
    pub fn quantified(expr: Ast, quantifier: Quantifier, greedy: bool) -> Self {
        Ast::Quantified {
            expr: Box::new(expr),
            quantifier,
            greedy,
        }
    }

    /// Create a capture group.
    #[must_use]
    pub fn group(index: usize, expr: Ast) -> Self {
        Ast::Group {
            index,
            name: None,
            expr: Box::new(expr),
        }
    }

    /// Create a named capture group.
    pub fn named_group(index: usize, name: impl Into<String>, expr: Ast) -> Self {
        Ast::Group {
            index,
            name: Some(name.into()),
            expr: Box::new(expr),
        }
    }

    /// Check if this AST is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Ast::Empty)
    }
}

/// Character class definition.
#[derive(Debug, Clone, PartialEq)]
pub struct CharClass {
    /// Whether this is a negated class `[^...]`.
    pub negated: bool,
    /// The ranges/characters in this class.
    pub items: Vec<CharClassItem>,
}

impl CharClass {
    /// Create a new character class.
    #[must_use]
    pub fn new(negated: bool, items: Vec<CharClassItem>) -> Self {
        CharClass { negated, items }
    }

    /// Create a character class matching any character except newlines (default `.`).
    #[must_use]
    pub fn any() -> Self {
        CharClass {
            negated: false,
            items: vec![CharClassItem::Named(NamedClass::AnyExceptNewline)],
        }
    }

    /// Create a character class matching any character including newlines (`dot_all` `.`).
    #[must_use]
    pub fn any_with_newlines() -> Self {
        CharClass {
            negated: false,
            items: vec![CharClassItem::Named(NamedClass::Any)],
        }
    }

    /// Create a digit class (`\d`).
    #[must_use]
    pub fn digit() -> Self {
        CharClass {
            negated: false,
            items: vec![CharClassItem::Named(NamedClass::Digit)],
        }
    }

    /// Create a word class (`\w`).
    #[must_use]
    pub fn word() -> Self {
        CharClass {
            negated: false,
            items: vec![CharClassItem::Named(NamedClass::Word)],
        }
    }

    /// Create a whitespace class (`\s`).
    #[must_use]
    pub fn whitespace() -> Self {
        CharClass {
            negated: false,
            items: vec![CharClassItem::Named(NamedClass::Whitespace)],
        }
    }

    /// Check if a character matches this class.
    #[must_use]
    pub fn matches(&self, ch: char) -> bool {
        let in_class = self.items.iter().any(|item| item.matches(ch));
        if self.negated { !in_class } else { in_class }
    }

    /// Check if a character matches this class with Unicode support.
    #[must_use]
    pub fn matches_unicode(&self, ch: char) -> bool {
        let in_class = self
            .items
            .iter()
            .any(|item| item.matches_with_unicode(ch, true));
        if self.negated { !in_class } else { in_class }
    }
}

/// An item in a character class.
#[derive(Debug, Clone, PartialEq)]
pub enum CharClassItem {
    /// Single character.
    Single(char),
    /// Character range: `a-z`.
    Range(char, char),
    /// Named character class: `\d`, `\w`, `\s`.
    Named(NamedClass),
}

impl CharClassItem {
    /// Check if a character matches this item.
    #[must_use]
    pub fn matches(&self, ch: char) -> bool {
        match self {
            CharClassItem::Single(c) => *c == ch,
            CharClassItem::Range(start, end) => ch >= *start && ch <= *end,
            CharClassItem::Named(class) => class.matches(ch),
        }
    }

    /// Check if a character matches this item with Unicode support.
    #[must_use]
    pub fn matches_with_unicode(&self, ch: char, unicode: bool) -> bool {
        match self {
            CharClassItem::Single(c) => *c == ch,
            CharClassItem::Range(start, end) => ch >= *start && ch <= *end,
            CharClassItem::Named(class) => class.matches_with_unicode(ch, unicode),
        }
    }
}

/// Named character class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedClass {
    /// `\d` - digits.
    Digit,
    /// `\D` - non-digits.
    NotDigit,
    /// `\w` - word characters.
    Word,
    /// `\W` - non-word characters.
    NotWord,
    /// `\s` - whitespace.
    Whitespace,
    /// `\S` - non-whitespace.
    NotWhitespace,
    /// `.` - any character (including newlines, for `dot_all` mode).
    Any,
    /// `.` - any character except newlines (default mode).
    AnyExceptNewline,
}

impl NamedClass {
    /// Check if a character matches this named class (ASCII mode).
    #[must_use]
    pub fn matches(&self, ch: char) -> bool {
        self.matches_with_unicode(ch, false)
    }

    /// Check if a character matches this named class.
    /// When `unicode` is true, uses Unicode character classes.
    #[must_use]
    pub fn matches_with_unicode(&self, ch: char, unicode: bool) -> bool {
        match self {
            NamedClass::Digit => {
                if unicode {
                    ch.is_ascii_digit() || unicode_digit(ch)
                } else {
                    ch.is_ascii_digit()
                }
            }
            NamedClass::NotDigit => {
                if unicode {
                    !(ch.is_ascii_digit() || unicode_digit(ch))
                } else {
                    !ch.is_ascii_digit()
                }
            }
            NamedClass::Word => {
                if unicode {
                    ch.is_alphanumeric() || ch == '_' || unicode_word_char(ch)
                } else {
                    ch.is_ascii_alphanumeric() || ch == '_'
                }
            }
            NamedClass::NotWord => {
                if unicode {
                    !(ch.is_alphanumeric() || ch == '_' || unicode_word_char(ch))
                } else {
                    !(ch.is_ascii_alphanumeric() || ch == '_')
                }
            }
            NamedClass::Whitespace => {
                if unicode {
                    ch.is_whitespace() || unicode_whitespace(ch)
                } else {
                    ch.is_ascii_whitespace()
                }
            }
            NamedClass::NotWhitespace => {
                if unicode {
                    !(ch.is_whitespace() || unicode_whitespace(ch))
                } else {
                    !ch.is_ascii_whitespace()
                }
            }
            NamedClass::Any => true,
            NamedClass::AnyExceptNewline => ch != '\n' && ch != '\r',
        }
    }
}

/// Check if character is a Unicode digit (outside ASCII).
fn unicode_digit(ch: char) -> bool {
    matches!(ch,
        '\u{0660}'..='\u{0669}' |  // Arabic-Indic digits
        '\u{06F0}'..='\u{06F9}' |  // Extended Arabic-Indic digits
        '\u{0966}'..='\u{096F}' |  // Devanagari digits
        '\u{0E50}'..='\u{0E59}' |  // Thai digits
        '\u{FF10}'..='\u{FF19}' |  // Fullwidth digits
        '\u{104A0}'..='\u{104D9}' | // Osage digits
        '\u{1D7CE}'..='\u{1D7FF}'  // Mathematical bold digits
    )
}

/// Check if character is a Unicode word character beyond ASCII.
fn unicode_word_char(ch: char) -> bool {
    // Check for Unicode letters and connectors
    matches!(ch,
        '\u{00C0}'..='\u{024F}' |  // Latin Extended
        '\u{0250}'..='\u{02AF}' |  // IPA Extensions
        '\u{02B0}'..='\u{02FF}' |  // Spacing Modifier Letters
        '\u{0300}'..='\u{036F}' |  // Combining Diacritical Marks
        '\u{0370}'..='\u{03FF}' |  // Greek
        '\u{0400}'..='\u{04FF}' |  // Cyrillic
        '\u{0500}'..='\u{052F}' |  // Cyrillic Supplement
        '\u{0530}'..='\u{058F}' |  // Armenian
        '\u{0590}'..='\u{05FF}' |  // Hebrew
        '\u{0600}'..='\u{06FF}' |  // Arabic
        '\u{0900}'..='\u{097F}' |  // Devanagari
        '\u{4E00}'..='\u{9FFF}' |  // CJK
        '\u{3400}'..='\u{4DBF}' |  // CJK Extension A
        '\u{F900}'..='\u{FAFF}' |  // CJK Compatibility
        '\u{2000}'..='\u{206F}' |  // General Punctuation
        '\u{2070}'..='\u{209F}' |  // Superscripts/Subscripts
        '\u{20A0}'..='\u{20CF}' |  // Currency Symbols
        '\u{2100}'..='\u{214F}' |  // Letterlike Symbols
        '\u{2150}'..='\u{218F}' |  // Number Forms
        '\u{2190}'..='\u{21FF}' |  // Arrows
        '\u{2200}'..='\u{22FF}' |  // Mathematical Operators
        '\u{2300}'..='\u{23FF}' |  // Miscellaneous Technical
        '\u{2460}'..='\u{24FF}' |  // Enclosed Alphanumerics
        '\u{2500}'..='\u{257F}' |  // Box Drawing
        '\u{2580}'..='\u{259F}' |  // Block Elements
        '\u{25A0}'..='\u{25FF}' |  // Geometric Shapes
        '\u{2600}'..='\u{26FF}' |  // Miscellaneous Symbols
        '\u{2700}'..='\u{27BF}' |  // Dingbats
        '\u{FB00}'..='\u{FB4F}' |  // Alphabetic Presentation Forms
        '\u{FB50}'..='\u{FDFF}' |  // Arabic Presentation Forms A
        '\u{FE70}'..='\u{FEFF}' |  // Arabic Presentation Forms B
        '\u{FF00}'..='\u{FFEF}' |  // Halfwidth/Fullwidth Forms
        '\u{1F600}'..='\u{1F64F}' | // Emoticons
        '\u{1F300}'..='\u{1F5FF}' | // Misc Symbols and Pictographs
        '\u{1F680}'..='\u{1F6FF}' | // Transport and Map
        '\u{1F900}'..='\u{1F9FF}' | // Supplemental Symbols
        '\u{1FA00}'..='\u{1FA6F}' | // Chess Symbols
        '\u{1FA70}'..='\u{1FAFF}' | // Symbols and Pictographs Extended
        '\u{1F170}'..='\u{1F19A}' | // Enclosed Alphanumeric Supplement
        '\u{00B5}' // Micro sign
    )
}

/// Check if character is Unicode whitespace beyond ASCII.
fn unicode_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0085}' |  // Next Line (NEL)
        '\u{00A0}' |  // No-Break Space
        '\u{1680}' |  // Ogham Space Mark
        '\u{2000}'
            ..='\u{200A}' |  // En Quad to Hair Space
        '\u{2028}' |  // Line Separator
        '\u{2029}' |  // Paragraph Separator
        '\u{202F}' |  // Narrow No-Break Space
        '\u{205F}' |  // Medium Mathematical Space
        '\u{3000}' // Ideographic Space
    )
}

/// Quantifier specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantifier {
    /// `*` - zero or more.
    ZeroOrMore,
    /// `+` - one or more.
    OneOrMore,
    /// `?` - zero or one.
    ZeroOrOne,
    /// `{n}` - exactly n.
    Exactly(usize),
    /// `{n,}` - at least n.
    AtLeast(usize),
    /// `{n,m}` - between n and m (inclusive).
    Between(usize, usize),
}

impl Quantifier {
    /// Get the minimum number of repetitions.
    #[must_use]
    pub fn min(&self) -> usize {
        match self {
            Quantifier::ZeroOrMore | Quantifier::ZeroOrOne => 0,
            Quantifier::OneOrMore => 1,
            Quantifier::Exactly(n) | Quantifier::AtLeast(n) | Quantifier::Between(n, _) => *n,
        }
    }

    /// Get the maximum number of repetitions (None = unbounded).
    #[must_use]
    pub fn max(&self) -> Option<usize> {
        match self {
            Quantifier::ZeroOrMore | Quantifier::OneOrMore | Quantifier::AtLeast(_) => None,
            Quantifier::ZeroOrOne => Some(1),
            Quantifier::Exactly(n) => Some(*n),
            Quantifier::Between(_, m) => Some(*m),
        }
    }
}

/// Anchor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// `^` - start of string/line.
    Start,
    /// `$` - end of string/line.
    End,
    /// `\b` - word boundary.
    WordBoundary,
    /// `\B` - non-word boundary.
    NotWordBoundary,
}
