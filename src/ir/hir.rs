//! High-level Intermediate Representation (HIR) for fuzzy regex.
//!
//! HIR is a simplified and normalized form of the AST that:
//! - Inlines non-capturing groups
//! - Normalizes character classes
//! - Resolves fuzziness inheritance
//! - Prepares for NFA construction

use crate::types::FuzzyLimits;

use crate::ir::EditCharRestriction;
use crate::parser::{Anchor, Ast, CharClass, CharClassItem, Fuzziness, MrabFuzziness, NamedClass};

/// Cost information for fuzzy matching.
#[derive(Debug, Clone, Default)]
pub struct CostInfo {
    /// Cost per insertion.
    pub insertion_cost: Option<u8>,
    /// Cost per deletion.
    pub deletion_cost: Option<u8>,
    /// Cost per substitution.
    pub substitution_cost: Option<u8>,
    /// Cost per transposition.
    pub transposition_cost: Option<u8>,
    /// Maximum total cost allowed.
    pub max_cost: Option<u8>,
}

impl CostInfo {
    /// Check if this cost info has any constraints.
    #[must_use]
    pub fn has_constraints(&self) -> bool {
        self.max_cost.is_some()
    }
}

/// HIR node representing a simplified regex pattern.
#[derive(Debug, Clone)]
pub enum Hir {
    /// Empty expression.
    Empty,

    /// A literal segment that can use fuzzy matching.
    Literal {
        /// The literal text to match.
        text: String,
        /// Fuzzy matching limits (insertions, deletions, substitutions).
        limits: Option<FuzzyLimits>,
        /// Minimum edits required (for exclusive lower bounds like `{0<e<5}`).
        min_edits: Option<u8>,
        /// Cost constraint info.
        cost_info: Option<CostInfo>,
        /// Character class restriction for edits (e.g., `{e<=1:[a-z]}`).
        edit_chars: Option<EditCharRestriction>,
    },

    /// A single character (exact match).
    Char(char),

    /// Character class (cannot use fuzzy matching directly).
    Class(HirClass),

    /// Character class with fuzzy matching support.
    /// Used when a character class is inside a fuzzy group like `(?:[a-z])~1`.
    FuzzyClass {
        /// The character class to match.
        class: HirClass,
        /// Fuzzy matching limits (insertions, deletions, substitutions).
        limits: Option<FuzzyLimits>,
        /// Minimum edits required (for exclusive lower bounds).
        min_edits: Option<u8>,
        /// Cost constraint info.
        cost_info: Option<CostInfo>,
    },

    /// Concatenation of expressions.
    Concat(Vec<Hir>),

    /// Alternation of expressions.
    Alt(Vec<Hir>),

    /// Repetition.
    Repeat {
        /// The expression to repeat.
        expr: Box<Hir>,
        /// Minimum number of repetitions.
        min: usize,
        /// Maximum number of repetitions (None means unbounded).
        max: Option<usize>,
        /// Whether the repetition is greedy.
        greedy: bool,
    },

    /// Capture group.
    Capture {
        /// The capture group index (1-based).
        index: usize,
        /// Optional name for named capture groups.
        name: Option<String>,
        /// The expression inside the capture group.
        expr: Box<Hir>,
    },

    /// Anchor.
    Anchor(Anchor),

    /// Lookahead assertion.
    Lookahead {
        /// Whether this is a positive lookahead (true) or negative lookahead (false).
        positive: bool,
        /// The expression to match in the lookahead.
        expr: Box<Hir>,
    },

    /// Lookbehind assertion.
    Lookbehind {
        /// Whether this is a positive lookbehind (true) or negative lookbehind (false).
        positive: bool,
        /// The expression to match in the lookbehind.
        expr: Box<Hir>,
    },

    /// Backreference to a capture group, optionally with fuzzy matching.
    Backreference {
        /// The capture group index to reference (1-based).
        group: usize,
        /// Fuzzy matching limits for the backreference.
        limits: Option<FuzzyLimits>,
    },

    /// Named list reference: \L<name>.
    /// This will be expanded to an alternation of the word list at match time.
    NamedList {
        /// The name of the word list.
        name: String,
    },

    /// Reset match start: \K
    /// Resets the starting point of the match. Everything before \K is matched
    /// but not included in the final match result.
    ResetMatchStart,

    /// Atomic group: (?>expr)
    /// Once the group matches, backtracking is disabled within the group.
    AtomicGroup {
        /// The expression contained in the atomic group.
        expr: Box<Hir>,
    },

    /// Recursive entire pattern: (?R)
    /// Recursively matches the entire pattern.
    RecursivePattern,

    /// Recursive numbered group: (?1), (?2), etc.
    RecursiveGroup {
        /// The capture group number to recurse into.
        group: usize,
    },

    /// Recursive named group: (?&name) or (?P>name)
    RecursiveNamedGroup {
        /// The name of the capture group to recurse into.
        name: String,
    },
}

impl Hir {
    /// Check if this HIR is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Hir::Empty)
    }

    /// Create a literal HIR node.
    pub fn literal(
        text: impl Into<String>,
        limits: Option<FuzzyLimits>,
        min_edits: Option<u8>,
        cost_info: Option<CostInfo>,
    ) -> Self {
        Hir::Literal {
            text: text.into(),
            limits,
            min_edits,
            cost_info,
            edit_chars: None,
        }
    }

    /// Create a literal HIR node with character class restriction.
    pub fn literal_with_edit_chars(
        text: impl Into<String>,
        limits: Option<FuzzyLimits>,
        min_edits: Option<u8>,
        cost_info: Option<CostInfo>,
        edit_chars: Option<EditCharRestriction>,
    ) -> Self {
        Hir::Literal {
            text: text.into(),
            limits,
            min_edits,
            cost_info,
            edit_chars,
        }
    }

    /// Create a character class HIR node.
    #[must_use]
    pub fn class(hir_class: HirClass) -> Self {
        Hir::Class(hir_class)
    }

    /// Create a repetition HIR node.
    #[must_use]
    pub fn repeat(expr: Hir, min: usize, max: Option<usize>, greedy: bool) -> Self {
        Hir::Repeat {
            expr: Box::new(expr),
            min,
            max,
            greedy,
        }
    }

    /// Create a capture group HIR node.
    #[must_use]
    pub fn capture(index: usize, name: Option<String>, expr: Hir) -> Self {
        Hir::Capture {
            index,
            name,
            expr: Box::new(expr),
        }
    }
}

/// Normalized character class for HIR.
#[derive(Debug, Clone)]
pub struct HirClass {
    /// Whether this is a negated class.
    pub negated: bool,
    /// Enumerated single characters.
    pub chars: Vec<char>,
    /// Character ranges (inclusive).
    pub ranges: Vec<(char, char)>,
    /// Named classes.
    pub named: Vec<NamedClass>,
}

impl HirClass {
    /// Create a new empty character class.
    #[must_use]
    pub fn new(negated: bool) -> Self {
        HirClass {
            negated,
            chars: Vec::new(),
            ranges: Vec::new(),
            named: Vec::new(),
        }
    }

    /// Create a class for any character except newlines (default `.`).
    #[must_use]
    pub fn any() -> Self {
        HirClass {
            negated: false,
            chars: Vec::new(),
            ranges: Vec::new(),
            named: vec![NamedClass::AnyExceptNewline],
        }
    }

    /// Create a class for any character including newlines (`dot_all` `.`).
    #[must_use]
    pub fn any_with_newlines() -> Self {
        HirClass {
            negated: false,
            chars: Vec::new(),
            ranges: Vec::new(),
            named: vec![NamedClass::Any],
        }
    }

    /// Add a single character.
    pub fn add_char(&mut self, ch: char) {
        self.chars.push(ch);
    }

    /// Add a character range.
    pub fn add_range(&mut self, start: char, end: char) {
        self.ranges.push((start, end));
    }

    /// Add a named class.
    pub fn add_named(&mut self, class: NamedClass) {
        self.named.push(class);
    }

    /// Check if a character matches this class.
    #[must_use]
    pub fn matches(&self, ch: char) -> bool {
        self.matches_with_unicode(ch, false)
    }

    /// Check if a character matches this class with unicode support.
    #[must_use]
    pub fn matches_with_unicode(&self, ch: char, unicode: bool) -> bool {
        let in_class = self.chars.contains(&ch)
            || self.ranges.iter().any(|&(s, e)| ch >= s && ch <= e)
            || self
                .named
                .iter()
                .any(|n| n.matches_with_unicode(ch, unicode));

        if self.negated { !in_class } else { in_class }
    }
}

impl From<&CharClass> for HirClass {
    fn from(class: &CharClass) -> Self {
        let mut hir = HirClass::new(class.negated);

        for item in &class.items {
            match item {
                CharClassItem::Single(ch) => hir.add_char(*ch),
                CharClassItem::Range(start, end) => hir.add_range(*start, *end),
                CharClassItem::Named(named) => hir.add_named(*named),
            }
        }

        hir
    }
}

/// Lower an AST to HIR.
pub struct HirLowering {
    /// Default edit count for inherited fuzziness.
    default_edits: u8,
    /// Unicode mode - enable Unicode character classes.
    unicode: bool,
}

impl HirLowering {
    /// Create a new HIR lowering pass with default fuzziness.
    #[must_use]
    pub fn new(default_edits: u8) -> Self {
        HirLowering {
            default_edits,
            unicode: false,
        }
    }

    /// Create a new HIR lowering pass with default fuzziness and unicode mode.
    #[must_use]
    pub fn new_with_unicode(default_edits: u8, unicode: bool) -> Self {
        HirLowering {
            default_edits,
            unicode,
        }
    }

    /// Lower an AST to HIR.
    #[must_use]
    pub fn lower(&self, ast: &Ast) -> Hir {
        self.lower_ast(ast)
    }

    /// Convert `CharClass` to `HirClass` with unicode mode.
    fn char_class_to_hir(&self, class: &CharClass) -> HirClass {
        let mut hir = HirClass::new(class.negated);

        for item in &class.items {
            match item {
                CharClassItem::Single(ch) => hir.add_char(*ch),
                CharClassItem::Range(start, end) => hir.add_range(*start, *end),
                CharClassItem::Named(named) => {
                    // In unicode mode, add unicode-aware ranges for named classes
                    if self.unicode {
                        Self::add_unicode_ranges(*named, &mut hir);
                    } else {
                        hir.add_named(*named);
                    }
                }
            }
        }

        hir
    }

    /// Add unicode-aware ranges for named character classes.
    fn add_unicode_ranges(named: NamedClass, hir: &mut HirClass) {
        match named {
            NamedClass::Word => {
                // ASCII word chars
                hir.add_range('a', 'z');
                hir.add_range('A', 'Z');
                hir.add_range('0', '9');
                hir.add_char('_');
                // Add common unicode word char ranges
                hir.add_range('\u{00C0}', '\u{024F}'); // Latin Extended
                hir.add_range('\u{0400}', '\u{04FF}'); // Cyrillic
                hir.add_range('\u{0900}', '\u{097F}'); // Devanagari
                hir.add_range('\u{4E00}', '\u{9FFF}'); // CJK
            }
            NamedClass::Digit => {
                hir.add_range('0', '9');
                // Add common unicode digit ranges
                hir.add_range('\u{0660}', '\u{0669}'); // Arabic-Indic
                hir.add_range('\u{0966}', '\u{096F}'); // Devanagari
            }
            NamedClass::Whitespace => {
                hir.add_char(' ');
                hir.add_char('\t');
                hir.add_char('\n');
                hir.add_char('\r');
                // Add unicode whitespace
                hir.add_char('\u{0085}'); // NEL
                hir.add_char('\u{00A0}'); // No-Break Space
                hir.add_range('\u{2000}', '\u{200A}'); // Spaces
            }
            NamedClass::NotWord => {
                // Negated: add all but word chars - add unicode ranges as negated
                // This is complex, so fall back to named class
                hir.add_named(named);
            }
            NamedClass::NotDigit
            | NamedClass::NotWhitespace
            | NamedClass::Any
            | NamedClass::AnyExceptNewline => {
                hir.add_named(named);
            }
        }
    }

    fn lower_ast(&self, ast: &Ast) -> Hir {
        match ast {
            Ast::Empty => Hir::Empty,

            Ast::Literal { text, fuzziness } => {
                let limits = fuzziness.to_limits(self.default_edits);
                let min_edits = fuzziness.min_edits();
                let cost_info = extract_cost_info(fuzziness);
                let edit_chars = extract_edit_chars(fuzziness);
                Hir::Literal {
                    text: text.clone(),
                    limits,
                    min_edits,
                    cost_info,
                    edit_chars,
                }
            }

            Ast::Char(ch) => Hir::Char(*ch),

            Ast::CharClass(class) => Hir::Class(self.char_class_to_hir(class)),

            Ast::Concat(parts) => {
                let lowered: Vec<_> = parts.iter().map(|p| self.lower_ast(p)).collect();
                Self::flatten_concat(lowered)
            }

            Ast::Alternation(alts) => {
                let lowered: Vec<_> = alts.iter().map(|a| self.lower_ast(a)).collect();
                if lowered.len() == 1 {
                    lowered.into_iter().next().unwrap()
                } else {
                    Hir::Alt(lowered)
                }
            }

            Ast::Quantified {
                expr,
                quantifier,
                greedy,
            } => {
                let inner = self.lower_ast(expr);
                let (min, max) = (quantifier.min(), quantifier.max());
                Hir::Repeat {
                    expr: Box::new(inner),
                    min,
                    max,
                    greedy: *greedy,
                }
            }

            Ast::Group { index, name, expr } => Hir::Capture {
                index: *index,
                name: name.clone(),
                expr: Box::new(self.lower_ast(expr)),
            },

            Ast::NonCapturingGroup { expr, fuzziness } => {
                // If the non-capturing group has its own fuzziness, apply it
                if matches!(fuzziness, Fuzziness::Inherited) {
                    // Just inline the contents
                    self.lower_ast(expr)
                } else {
                    self.lower_with_fuzziness(expr, fuzziness)
                }
            }

            Ast::Anchor(anchor) => Hir::Anchor(*anchor),

            Ast::Lookahead { positive, expr } => Hir::Lookahead {
                positive: *positive,
                expr: Box::new(self.lower_ast(expr)),
            },

            Ast::Lookbehind { positive, expr } => Hir::Lookbehind {
                positive: *positive,
                expr: Box::new(self.lower_ast(expr)),
            },

            Ast::Backreference { group, fuzziness } => {
                let limits = fuzziness.to_limits(self.default_edits);
                Hir::Backreference {
                    group: *group,
                    limits,
                }
            }

            Ast::NamedList { name } => {
                // NamedList will be expanded later when the word list is provided
                // For now, create a placeholder that will be resolved at match time
                Hir::NamedList { name: name.clone() }
            }

            Ast::ResetMatchStart => {
                // \K resets the match start position
                Hir::ResetMatchStart
            }

            Ast::AtomicGroup { expr } => {
                // Atomic group - lower the inner expression
                Hir::AtomicGroup {
                    expr: Box::new(self.lower_ast(expr)),
                }
            }

            Ast::RecursivePattern => {
                // (?R) - recursively match the entire pattern
                // This will be resolved during NFA construction
                Hir::RecursivePattern
            }

            Ast::RecursiveGroup { group } => {
                // (?1), (?2), etc. - recursively match a capture group
                Hir::RecursiveGroup { group: *group }
            }

            Ast::RecursiveNamedGroup { name } => {
                // (?&name) or (?P>name) - recursively match a named capture group
                Hir::RecursiveNamedGroup { name: name.clone() }
            }
        }
    }

    /// Lower an expression with specific fuzziness override.
    fn lower_with_fuzziness(&self, ast: &Ast, fuzziness: &Fuzziness) -> Hir {
        // Lower with the new default, but for detailed/mrab limits, inject them directly
        match fuzziness {
            Fuzziness::Exact => {
                // Exact match - no edits allowed
                let lowering = HirLowering::new(0);
                lowering.lower_ast(ast)
            }
            Fuzziness::Edits(n) => {
                // Simple edit count - convert to FuzzyLimits and use detailed path
                // This ensures character classes get FuzzyClass treatment
                let limits = FuzzyLimits::new().edits(*n);
                self.lower_with_detailed_limits(ast, &limits, None, None, None)
            }
            Fuzziness::Detailed(limits) => {
                self.lower_with_detailed_limits(ast, limits, None, None, None)
            }
            Fuzziness::MrabStyle(mrab) => {
                let limits = mrab.to_limits();
                let min_edits = mrab.min_errors;
                let cost_info = extract_cost_info_from_mrab(mrab);
                let edit_chars = extract_edit_chars_from_mrab(mrab);
                self.lower_with_detailed_limits(ast, &limits, min_edits, cost_info, edit_chars)
            }
            Fuzziness::Inherited => {
                // Use the default edits from self
                let lowering = HirLowering::new(self.default_edits);
                lowering.lower_ast(ast)
            }
        }
    }

    /// Lower an expression with detailed fuzzy limits applied to all literals.
    fn lower_with_detailed_limits(
        &self,
        ast: &Ast,
        limits: &FuzzyLimits,
        min_edits: Option<u8>,
        cost_info: Option<CostInfo>,
        edit_chars: Option<EditCharRestriction>,
    ) -> Hir {
        match ast {
            Ast::Literal { text, .. } => Hir::Literal {
                text: text.clone(),
                limits: Some(limits.clone()),
                min_edits,
                cost_info: cost_info.clone(),
                edit_chars: edit_chars.clone(),
            },

            Ast::Char(ch) => Hir::Literal {
                text: ch.to_string(),
                limits: Some(limits.clone()),
                min_edits,
                cost_info: cost_info.clone(),
                edit_chars: edit_chars.clone(),
            },

            Ast::Concat(parts) => {
                let lowered: Vec<_> = parts
                    .iter()
                    .map(|p| {
                        self.lower_with_detailed_limits(
                            p,
                            limits,
                            min_edits,
                            cost_info.clone(),
                            edit_chars.clone(),
                        )
                    })
                    .collect();
                Self::flatten_concat(lowered)
            }

            Ast::Alternation(alts) => {
                let lowered: Vec<_> = alts
                    .iter()
                    .map(|a| {
                        self.lower_with_detailed_limits(
                            a,
                            limits,
                            min_edits,
                            cost_info.clone(),
                            edit_chars.clone(),
                        )
                    })
                    .collect();
                Hir::Alt(lowered)
            }

            Ast::Quantified {
                expr,
                quantifier,
                greedy,
            } => {
                let inner =
                    self.lower_with_detailed_limits(expr, limits, min_edits, cost_info, edit_chars);
                Hir::Repeat {
                    expr: Box::new(inner),
                    min: quantifier.min(),
                    max: quantifier.max(),
                    greedy: *greedy,
                }
            }

            Ast::Group { index, name, expr } => Hir::Capture {
                index: *index,
                name: name.clone(),
                expr: Box::new(
                    self.lower_with_detailed_limits(expr, limits, min_edits, cost_info, edit_chars),
                ),
            },

            Ast::NonCapturingGroup { expr, .. } => {
                self.lower_with_detailed_limits(expr, limits, min_edits, cost_info, edit_chars)
            }

            // Character class with fuzzy limits becomes FuzzyClass
            Ast::CharClass(class) => Hir::FuzzyClass {
                class: self.char_class_to_hir(class),
                limits: Some(limits.clone()),
                min_edits,
                cost_info,
            },

            // Backreference inside fuzzy group gets the fuzzy limits
            Ast::Backreference { group, fuzziness } => {
                // If the backref already has its own fuzziness, use that; otherwise inherit from group
                let backref_limits = fuzziness
                    .to_limits(self.default_edits)
                    .or_else(|| Some(limits.clone()));
                Hir::Backreference {
                    group: *group,
                    limits: backref_limits,
                }
            }

            // Other nodes don't need fuzziness applied
            other => self.lower_ast(other),
        }
    }

    /// Flatten nested concats and filter empties.
    fn flatten_concat(parts: Vec<Hir>) -> Hir {
        let mut result = Vec::new();

        for part in parts {
            match part {
                Hir::Empty => {}
                Hir::Concat(inner) => result.extend(inner),
                other => result.push(other),
            }
        }

        match result.len() {
            0 => Hir::Empty,
            1 => result.pop().unwrap(),
            _ => Hir::Concat(result),
        }
    }
}

/// Extract cost info from fuzziness specification.
fn extract_cost_info(fuzziness: &Fuzziness) -> Option<CostInfo> {
    match fuzziness {
        Fuzziness::MrabStyle(mrab) => {
            if mrab.max_cost.is_some() {
                Some(CostInfo {
                    insertion_cost: mrab.insertion_cost,
                    deletion_cost: mrab.deletion_cost,
                    substitution_cost: mrab.substitution_cost,
                    transposition_cost: mrab.transposition_cost,
                    max_cost: mrab.max_cost,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract cost info from `MrabFuzziness`.
fn extract_cost_info_from_mrab(mrab: &MrabFuzziness) -> Option<CostInfo> {
    if mrab.max_cost.is_some() {
        Some(CostInfo {
            insertion_cost: mrab.insertion_cost,
            deletion_cost: mrab.deletion_cost,
            substitution_cost: mrab.substitution_cost,
            transposition_cost: mrab.transposition_cost,
            max_cost: mrab.max_cost,
        })
    } else {
        None
    }
}

/// Extract edit character restriction from fuzziness specification.
fn extract_edit_chars(fuzziness: &Fuzziness) -> Option<EditCharRestriction> {
    match fuzziness {
        Fuzziness::MrabStyle(mrab) => extract_edit_chars_from_mrab(mrab),
        _ => None,
    }
}

/// Extract edit character restriction from `MrabFuzziness`.
/// Combines all character class restrictions (substitution, insertion, deletion)
/// into a single restriction since mrab-regex syntax like `{e<=1:[a-z]}` applies to all edit types.
fn extract_edit_chars_from_mrab(mrab: &MrabFuzziness) -> Option<EditCharRestriction> {
    // Check for any character class restriction
    // In mrab-regex, `{e<=1:[a-z]}` applies the class to all edit types
    let char_class = mrab
        .substitution_chars
        .as_ref()
        .or(mrab.insertion_chars.as_ref())
        .or(mrab.deletion_chars.as_ref())?;

    let mut chars = Vec::new();
    let mut ranges = Vec::new();

    for item in &char_class.items {
        match item {
            CharClassItem::Single(ch) => chars.push(*ch),
            CharClassItem::Range(start, end) => ranges.push((*start, *end)),
            CharClassItem::Named(named) => {
                // Expand named classes to ranges
                match named {
                    NamedClass::Digit => ranges.push(('0', '9')),
                    NamedClass::Word => {
                        ranges.push(('a', 'z'));
                        ranges.push(('A', 'Z'));
                        ranges.push(('0', '9'));
                        chars.push('_');
                    }
                    NamedClass::Whitespace => {
                        chars.extend([' ', '\t', '\n', '\r']);
                    }
                    _ => {} // Skip negated and Any classes
                }
            }
        }
    }

    if chars.is_empty() && ranges.is_empty() {
        None
    } else {
        Some(EditCharRestriction::new(chars, ranges))
    }
}

/// Lower an AST to HIR with default fuzziness.
#[must_use]
pub fn lower(ast: &Ast, default_edits: u8) -> Hir {
    lower_with_unicode(ast, default_edits, false)
}

/// Lower an AST to HIR with unicode mode.
#[must_use]
pub fn lower_with_unicode(ast: &Ast, default_edits: u8, unicode: bool) -> Hir {
    HirLowering::new_with_unicode(default_edits, unicode).lower(ast)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn test_lower_literal() {
        let ast = parse("hello").unwrap();
        let hir = lower(&ast, 2);
        match hir {
            Hir::Literal { text, limits, .. } => {
                assert_eq!(text, "hello");
                assert!(limits.is_some());
            }
            _ => panic!("expected literal"),
        }
    }

    #[test]
    fn test_lower_literal_exact() {
        let ast = parse("hello~0").unwrap();
        let hir = lower(&ast, 2);
        match hir {
            Hir::Literal { text, limits, .. } => {
                assert_eq!(text, "hello");
                // limits should allow 0 edits
                assert!(limits.is_some());
            }
            _ => panic!("expected literal"),
        }
    }

    #[test]
    fn test_lower_concat() {
        let ast = parse("ab").unwrap();
        let hir = lower(&ast, 0);
        assert!(matches!(hir, Hir::Literal { .. }));
    }

    #[test]
    fn test_lower_alternation() {
        let ast = parse("a|b").unwrap();
        let hir = lower(&ast, 0);
        assert!(matches!(hir, Hir::Alt(_)));
    }

    #[test]
    fn test_lower_quantifier() {
        let ast = parse("a+").unwrap();
        let hir = lower(&ast, 0);
        match hir {
            Hir::Repeat { min, max, .. } => {
                assert_eq!(min, 1);
                assert_eq!(max, None);
            }
            _ => panic!("expected repeat"),
        }
    }

    #[test]
    fn test_lower_group() {
        let ast = parse("(abc)").unwrap();
        let hir = lower(&ast, 0);
        assert!(matches!(hir, Hir::Capture { index: 1, .. }));
    }

    #[test]
    fn test_lower_non_capturing_inlined() {
        let ast = parse("(?:abc)").unwrap();
        let hir = lower(&ast, 0);
        // Non-capturing group should be inlined
        assert!(matches!(hir, Hir::Literal { .. }));
    }
}
