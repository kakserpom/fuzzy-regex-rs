//! Recursive descent parser for fuzzy regex patterns.

#![allow(
    clippy::redundant_else,
    clippy::missing_errors_doc,
    clippy::match_same_arms,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::float_cmp,
    clippy::allow_attributes,
    let_underscore_drop
)]

use crate::types::FuzzyLimits;

use super::ast::{
    Anchor, Ast, CharClass, CharClassItem, Fuzziness, MatchFlags, MrabFuzziness, NamedClass,
    Quantifier,
};
use super::lexer::{Lexer, NamedClassToken, Token};
use crate::error::{Error, Result};

/// Parser for fuzzy regex patterns.
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    capture_count: usize,
    /// Total number of capture groups in the whole pattern, counted by a
    /// pre-scan. Lets a backreference resolve to a group defined *later*
    /// (a forward reference, e.g. `\1(a)` or `(?r)\1(a)`).
    total_captures: usize,
    /// All capture-group names mapped to their 1-based index, from the pre-scan.
    /// Used to resolve named backreferences (`\k<name>`, `(?P=name)`), including
    /// forward references to a name defined later in the pattern.
    capture_names: std::collections::HashMap<String, usize>,
    /// Match flags accumulated during parsing.
    flags: MatchFlags,
}

/// Pre-scan the pattern to count capture groups and collect their names, in the
/// same order the parser assigns indices (each capturing `(` / `(?<name>` opens
/// the next group). Best-effort: a lex error just stops the scan and lets the
/// main parse report it properly. Enables forward references, which need the
/// group count/names known before the referencing token is parsed.
fn prescan_captures(
    pattern: &str,
    verbose: bool,
) -> (usize, std::collections::HashMap<String, usize>) {
    let mut lexer = Lexer::new_with_flags(pattern, verbose);
    let mut count = 0usize;
    let mut names = std::collections::HashMap::new();
    loop {
        match lexer.next_token() {
            Ok(Token::Eof) | Err(_) => break,
            Ok(Token::OpenParen) => count += 1,
            Ok(Token::NamedGroup(name)) => {
                count += 1;
                names.entry(name).or_insert(count);
            }
            Ok(_) => {}
        }
    }
    (count, names)
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given pattern.
    pub fn new(pattern: &'a str) -> Self {
        let (total_captures, capture_names) = prescan_captures(pattern, false);
        Parser {
            lexer: Lexer::new(pattern),
            capture_count: 0,
            total_captures,
            capture_names,
            flags: MatchFlags::default(),
        }
    }

    /// Create a new parser with initial flags.
    pub fn new_with_flags(pattern: &'a str, verbose: bool, dot_all: bool, ungreedy: bool) -> Self {
        let (total_captures, capture_names) = prescan_captures(pattern, verbose);
        Parser {
            lexer: Lexer::new_with_flags(pattern, verbose),
            capture_count: 0,
            total_captures,
            capture_names,
            flags: MatchFlags {
                verbose,
                dot_all,
                ungreedy,
                ..Default::default()
            },
        }
    }

    /// Parse the pattern into an AST.
    pub fn parse(mut self) -> Result<Ast> {
        let ast = self.parse_alternation()?;

        if !matches!(self.lexer.peek_token()?, Token::Eof) {
            return Err(Error::parse(
                self.lexer.position(),
                "unexpected token after pattern",
            ));
        }

        Ok(ast)
    }

    /// Get the number of capture groups found.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn capture_count(&self) -> usize {
        self.capture_count
    }

    /// Get the match flags found in the pattern.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn flags(&self) -> MatchFlags {
        self.flags
    }

    /// Parse alternation: expr | expr | ...
    fn parse_alternation(&mut self) -> Result<Ast> {
        let mut alternatives = vec![self.parse_concat()?];

        while matches!(self.lexer.peek_token()?, Token::Pipe) {
            self.lexer.next_token()?; // consume '|'
            alternatives.push(self.parse_concat()?);
        }

        if alternatives.len() == 1 {
            Ok(alternatives.pop().unwrap())
        } else {
            Ok(Ast::Alternation(alternatives))
        }
    }

    /// Parse concatenation: expr expr expr ...
    fn parse_concat(&mut self) -> Result<Ast> {
        let mut parts = Vec::new();

        loop {
            match self.lexer.peek_token()? {
                Token::Eof | Token::Pipe | Token::CloseParen => break,
                _ => parts.push(self.parse_quantified()?),
            }
        }

        match parts.len() {
            0 => Ok(Ast::Empty),
            1 => Ok(parts.pop().unwrap()),
            _ => {
                // Merge adjacent literals that don't have their own fuzziness
                let merged = Self::merge_literals(parts);
                if merged.len() == 1 {
                    Ok(merged.into_iter().next().unwrap())
                } else {
                    Ok(Ast::Concat(merged))
                }
            }
        }
    }

    /// Merge adjacent literals/chars into single literal nodes.
    /// If the last merged element has fuzziness, apply it to the whole merged literal.
    fn merge_literals(parts: Vec<Ast>) -> Vec<Ast> {
        let mut result = Vec::new();
        let mut current_literal = String::new();
        let mut pending_fuzziness: Option<Fuzziness> = None;

        for part in parts {
            match part {
                Ast::Char(ch) => {
                    // If we had pending fuzziness from a previous literal, flush first
                    if pending_fuzziness.is_some() {
                        let fuzz = pending_fuzziness.take().unwrap();
                        result.push(Ast::Literal {
                            text: std::mem::take(&mut current_literal),
                            fuzziness: fuzz,
                        });
                    }
                    current_literal.push(ch);
                }
                Ast::Literal { text, fuzziness } => {
                    if fuzziness == Fuzziness::Inherited {
                        // If we had pending fuzziness, flush first
                        if pending_fuzziness.is_some() {
                            let fuzz = pending_fuzziness.take().unwrap();
                            result.push(Ast::Literal {
                                text: std::mem::take(&mut current_literal),
                                fuzziness: fuzz,
                            });
                        }
                        current_literal.push_str(&text);
                    } else {
                        // This literal has its own fuzziness
                        // Merge accumulated chars with this text and apply fuzziness
                        current_literal.push_str(&text);
                        pending_fuzziness = Some(fuzziness);
                    }
                }
                other => {
                    // Flush current literal
                    if !current_literal.is_empty() {
                        let fuzz = pending_fuzziness.take().unwrap_or(Fuzziness::Inherited);
                        result.push(Ast::Literal {
                            text: std::mem::take(&mut current_literal),
                            fuzziness: fuzz,
                        });
                    }
                    result.push(other);
                }
            }
        }

        // Flush remaining literal
        if !current_literal.is_empty() {
            let fuzz = pending_fuzziness.unwrap_or(Fuzziness::Inherited);
            result.push(Ast::Literal {
                text: current_literal,
                fuzziness: fuzz,
            });
        }

        result
    }

    /// Parse quantified expression: atom (quantifier)?
    fn parse_quantified(&mut self) -> Result<Ast> {
        let mut expr = self.parse_atom()?;

        // Check for fuzziness marker on literals
        if matches!(&expr, Ast::Literal { .. } | Ast::Char(_))
            && matches!(self.lexer.peek_token()?, Token::Tilde)
        {
            self.lexer.next_token()?; // consume '~'
            let fuzziness = self.parse_fuzziness()?;

            let text = match expr {
                Ast::Literal { text, .. } => text,
                Ast::Char(ch) => ch.to_string(),
                _ => unreachable!(),
            };

            expr = Ast::Literal { text, fuzziness };
        }

        // Check for quantifier
        match self.lexer.peek_token()? {
            Token::Star | Token::Plus | Token::Question | Token::OpenBrace => {
                let (quantifier, greedy) = self.parse_quantifier()?;
                Ok(Ast::quantified(expr, quantifier, greedy))
            }
            _ => Ok(expr),
        }
    }

    /// Parse a quantifier.
    fn parse_quantifier(&mut self) -> Result<(Quantifier, bool)> {
        // First consume the quantifier token to check what's after it
        let token = self.lexer.next_token()?;

        // Check for possessive by looking at what follows the quantifier
        // This handles cases like ".*+?" where *+ should NOT be possessive
        // because the + is followed by ? (which is invalid - produces error in Python)
        let is_possessive = match &token {
            Token::Star | Token::Plus | Token::Question => {
                // Look ahead: possessive only if we see + NOT followed by ?
                if matches!(self.lexer.peek_token()?, Token::Plus) {
                    // Need to check if + is followed by ? (which would be an error)
                    // Save position to peek further
                    let pos = self.lexer.position();
                    self.lexer.next_token()?; // consume +
                    let followed_by_question = matches!(self.lexer.peek_token()?, Token::Question);
                    // Restore position for error reporting
                    self.lexer.set_position(pos);
                    if followed_by_question {
                        return Err(Error::parse(
                            self.lexer.position(),
                            "possessive quantifier cannot be followed by '?'",
                        ));
                    }
                    true // Possessive
                } else {
                    false
                }
            }
            _ => false,
        };

        let quantifier = match token {
            Token::Star => {
                if is_possessive {
                    self.lexer.next_token()?; // consume the extra +
                    Quantifier::ZeroOrMore
                } else {
                    Quantifier::ZeroOrMore
                }
            }
            Token::Plus => {
                if is_possessive {
                    self.lexer.next_token()?; // consume the extra +
                    Quantifier::OneOrMore
                } else {
                    Quantifier::OneOrMore
                }
            }
            Token::Question => {
                if is_possessive {
                    self.lexer.next_token()?; // consume the extra +
                    Quantifier::ZeroOrOne
                } else {
                    Quantifier::ZeroOrOne
                }
            }
            Token::OpenBrace => self.parse_brace_quantifier()?,
            _ => unreachable!(),
        };

        // Check for greediness modifier
        // Without modifier: greedy by default (or non-greedy if ungreedy flag is set)
        // With `?` modifier: inverts the default
        let has_modifier = matches!(self.lexer.peek_token()?, Token::Question);
        if has_modifier {
            self.lexer.next_token()?;
        }

        // In ungreedy mode, the meaning is inverted:
        // - No modifier = non-greedy
        // - With `?` = greedy
        let greedy = if self.flags.ungreedy {
            has_modifier // `?` makes it greedy in ungreedy mode
        } else {
            !has_modifier // `?` makes it non-greedy in normal mode
        };

        Ok((quantifier, greedy))
    }

    /// Parse a brace quantifier: {n}, {n,}, {n,m}
    fn parse_brace_quantifier(&mut self) -> Result<Quantifier> {
        let start_pos = self.lexer.position();

        // Parse first number
        let min = self.parse_number_usize()?;

        match self.lexer.peek_token()? {
            Token::CloseBrace => {
                self.lexer.next_token()?;
                Ok(Quantifier::Exactly(min))
            }
            Token::Char(',') => {
                self.lexer.next_token()?; // consume ','

                if self.lexer.peek_token()? == Token::CloseBrace {
                    self.lexer.next_token()?;
                    Ok(Quantifier::AtLeast(min))
                } else {
                    let max = self.parse_number_usize()?;
                    if max < min {
                        return Err(Error::invalid_quantifier(
                            start_pos,
                            format!("max ({max}) cannot be less than min ({min})"),
                        ));
                    }
                    match self.lexer.next_token()? {
                        Token::CloseBrace => Ok(Quantifier::Between(min, max)),
                        _ => Err(Error::unclosed("quantifier", start_pos)),
                    }
                }
            }
            _ => Err(Error::invalid_quantifier(
                start_pos,
                "expected ',' or '}' in quantifier",
            )),
        }
    }

    /// Parse a number from consecutive digits as usize.
    fn parse_number_usize(&mut self) -> Result<usize> {
        let start_pos = self.lexer.position();
        let mut num = 0usize;
        let mut found_digit = false;

        while let Token::Char(ch) = self.lexer.peek_token()? {
            let Some(digit) = ch.to_digit(10).and_then(|x| u8::try_from(x).ok()) else {
                break;
            };
            found_digit = true;
            num = num
                .checked_mul(10)
                .and_then(|n| n.checked_add(digit as usize))
                .ok_or_else(|| Error::invalid_quantifier(start_pos, "number too large"))?;
            self.lexer.next_token()?;
        }

        if !found_digit {
            return Err(Error::invalid_quantifier(start_pos, "expected number"));
        }

        Ok(num)
    }

    /// Parse a number from consecutive digits as u8.
    fn parse_number_u8(&mut self) -> Result<u8> {
        let start_pos = self.lexer.position();
        let mut num = 0u8;
        let mut found_digit = false;

        while let Token::Char(ch) = self.lexer.peek_token()? {
            let digit = ch.to_string().parse::<u8>().ok();
            let Some(digit) = digit else {
                break;
            };
            found_digit = true;
            num = num
                .checked_mul(10)
                .and_then(|n| n.checked_add(digit))
                .ok_or_else(|| Error::invalid_quantifier(start_pos, "number too large"))?;
            self.lexer.next_token()?;
        }

        if !found_digit {
            return Err(Error::invalid_quantifier(start_pos, "expected number"));
        }

        Ok(num)
    }

    /// Parse fuzziness specification after '~'.
    fn parse_fuzziness(&mut self) -> Result<Fuzziness> {
        let start_pos = self.lexer.position();

        match self.lexer.peek_token()? {
            Token::OpenBrace => {
                self.lexer.next_token()?; // consume '{'
                let limits = self.parse_detailed_fuzziness()?;
                Ok(Fuzziness::Detailed(limits))
            }
            Token::Char(ch) if ch.is_ascii_digit() => {
                let num = self.parse_number_u8()?;
                if num == 0 {
                    Ok(Fuzziness::Exact)
                } else {
                    Ok(Fuzziness::Edits(num))
                }
            }
            _ => Err(Error::invalid_fuzziness(
                start_pos,
                "expected number or '{' after '~'",
            )),
        }
    }

    /// Parse detailed fuzziness: ~{i=1,d=2,s=0,e=3}
    fn parse_detailed_fuzziness(&mut self) -> Result<FuzzyLimits> {
        let start_pos = self.lexer.position();
        let mut limits = FuzzyLimits::new();

        loop {
            // Parse key (i, d, s, w, e)
            let key = match self.lexer.next_token()? {
                Token::Char(ch) => ch,
                Token::CloseBrace => break,
                _ => {
                    return Err(Error::invalid_fuzziness(
                        start_pos,
                        "expected fuzziness key (i, d, s, w, e)",
                    ));
                }
            };

            // Expect '='
            match self.lexer.next_token()? {
                Token::Char('=') => {}
                _ => {
                    return Err(Error::invalid_fuzziness(
                        start_pos,
                        "expected '=' after fuzziness key",
                    ));
                }
            }

            // Parse value
            let value = self.parse_number_u8()?;

            // Apply to limits
            limits = match key {
                'i' => limits.insertions(value),
                'd' => limits.deletions(value),
                's' => limits.substitutions(value),
                'w' => limits.swaps(value),
                'e' => limits.edits(value),
                _ => {
                    return Err(Error::invalid_fuzziness(
                        start_pos,
                        format!("unknown fuzziness key: '{key}'"),
                    ));
                }
            };

            // Check for comma or closing brace
            match self.lexer.peek_token()? {
                Token::Char(',') => {
                    self.lexer.next_token()?;
                }
                Token::CloseBrace => {
                    self.lexer.next_token()?;
                    break;
                }
                _ => {
                    return Err(Error::invalid_fuzziness(
                        start_pos,
                        "expected ',' or '}' in fuzziness specification",
                    ));
                }
            }
        }

        Ok(limits)
    }

    /// Parse an atom (base expression).
    fn parse_atom(&mut self) -> Result<Ast> {
        match self.lexer.next_token()? {
            Token::Char(ch) | Token::Escaped(ch) => Ok(Ast::Char(ch)),
            Token::Dot => {
                if self.flags.dot_all {
                    Ok(Ast::CharClass(CharClass::any_with_newlines()))
                } else {
                    Ok(Ast::CharClass(CharClass::any()))
                }
            }
            // Hyphen is a regular character outside of character classes
            Token::Hyphen => Ok(Ast::Char('-')),

            Token::NamedClass(class) => Ok(Self::parse_named_class(class)),

            Token::OpenParen => self.parse_capture_group(),
            Token::NonCapturing => self.parse_non_capturing_group(),
            Token::PositiveLookahead => self.parse_lookahead(true),
            Token::NegativeLookahead => self.parse_lookahead(false),
            Token::PositiveLookbehind => self.parse_lookbehind(true),
            Token::NegativeLookbehind => self.parse_lookbehind(false),
            Token::NamedGroup(name) => self.parse_named_capture_group(name),

            // Match flags - set flag and continue parsing
            Token::BestMatch => {
                self.flags.best_match = true;
                // Return empty and let concatenation handle it
                Ok(Ast::Empty)
            }
            Token::EnhanceMatch => {
                self.flags.enhance_match = true;
                // Return empty and let concatenation handle it
                Ok(Ast::Empty)
            }
            Token::PosixMatch => {
                self.flags.posix = true;
                // Return empty and let concatenation handle it
                Ok(Ast::Empty)
            }
            Token::Verbose => {
                self.flags.verbose = true;
                Ok(Ast::Empty)
            }
            Token::DotAll => {
                self.flags.dot_all = true;
                Ok(Ast::Empty)
            }
            Token::MultiLine => {
                self.flags.multi_line = true;
                Ok(Ast::Empty)
            }
            Token::Ungreedy => {
                self.flags.ungreedy = true;
                Ok(Ast::Empty)
            }
            Token::CaseInsensitive => {
                self.flags.case_insensitive = true;
                Ok(Ast::Empty)
            }
            Token::Global => {
                self.flags.global = true;
                Ok(Ast::Empty)
            }
            Token::Unicode => {
                self.flags.unicode = true;
                Ok(Ast::Empty)
            }

            // A run of inline flags, e.g. `(?es)`, `(?ims)`. Apply each set flag.
            Token::InlineFlags(f) => {
                self.flags.best_match |= f.best_match;
                self.flags.enhance_match |= f.enhance_match;
                self.flags.posix |= f.posix;
                self.flags.verbose |= f.verbose;
                self.flags.dot_all |= f.dot_all;
                self.flags.multi_line |= f.multi_line;
                self.flags.ungreedy |= f.ungreedy;
                self.flags.case_insensitive |= f.case_insensitive;
                self.flags.global |= f.global;
                self.flags.unicode |= f.unicode;
                self.flags.reverse |= f.reverse;
                Ok(Ast::Empty)
            }

            Token::OpenBracket => self.parse_char_class(),

            Token::Caret => Ok(Ast::Anchor(Anchor::Start)),
            Token::Dollar => Ok(Ast::Anchor(Anchor::End)),

            Token::Backreference(n) => {
                // Validate against the total group count (from the pre-scan), not
                // just groups seen so far, so a forward reference to a group
                // defined later in the pattern is accepted. A reference to a group
                // that never exists is still an error.
                if n == 0 || n > self.total_captures {
                    Err(Error::invalid_backreference(
                        n,
                        self.lexer.position(),
                        format!("backreference to group {n} that does not exist"),
                    ))
                } else {
                    let fuzziness = self.parse_optional_backref_fuzziness()?;
                    Ok(Ast::Backreference {
                        group: n,
                        fuzziness,
                    })
                }
            }

            Token::NamedBackreference(name) => {
                let Some(&group) = self.capture_names.get(&name) else {
                    return Err(Error::parse(
                        self.lexer.position(),
                        format!("backreference to unknown group name '{name}'"),
                    ));
                };
                let fuzziness = self.parse_optional_backref_fuzziness()?;
                Ok(Ast::Backreference { group, fuzziness })
            }

            Token::NamedList(name) => {
                // Check for optional fuzziness specifier {e<=1}
                let fuzziness = if matches!(self.lexer.peek_token()?, Token::OpenBrace) {
                    // Need to check if this is mrab-style fuzziness vs quantifier
                    if self.peek_is_mrab_fuzziness() {
                        self.lexer.next_token()?; // consume '{'
                        self.parse_mrab_fuzziness()?
                    } else {
                        Fuzziness::Inherited
                    }
                } else {
                    Fuzziness::Inherited
                };

                // Wrap NamedList in a NonCapturingGroup with fuzziness
                Ok(Ast::NonCapturingGroup {
                    expr: Box::new(Ast::NamedList { name: name.clone() }),
                    fuzziness,
                })
            }

            Token::ResetMatchStart => {
                // \K resets the match start position
                Ok(Ast::ResetMatchStart)
            }

            Token::AtomicGroup => self.parse_atomic_group(),

            Token::RecursivePattern => {
                // (?R) - recursively match the entire pattern
                Ok(Ast::RecursivePattern)
            }

            Token::RecursiveGroup(group) => {
                // (?1), (?2), etc. - recursively match a capture group
                Ok(Ast::RecursiveGroup { group })
            }

            Token::RecursiveNamedGroup(name) => {
                // (?&name) or (?P>name) - recursively match a named capture group
                Ok(Ast::RecursiveNamedGroup { name })
            }

            Token::Handler(name) => {
                // (?call:name) - invoke a custom handler
                Ok(Ast::Handler { name })
            }

            token => Err(Error::parse(
                self.lexer.position(),
                format!("unexpected token: {token:?}"),
            )),
        }
    }

    /// Parse a named class escape.
    fn parse_named_class(class: NamedClassToken) -> Ast {
        match class {
            NamedClassToken::Digit => Ast::CharClass(CharClass::digit()),
            NamedClassToken::NotDigit => Ast::CharClass(CharClass::new(
                true,
                vec![CharClassItem::Named(NamedClass::Digit)],
            )),
            NamedClassToken::Word => Ast::CharClass(CharClass::word()),
            NamedClassToken::NotWord => Ast::CharClass(CharClass::new(
                true,
                vec![CharClassItem::Named(NamedClass::Word)],
            )),
            NamedClassToken::Whitespace => Ast::CharClass(CharClass::whitespace()),
            NamedClassToken::NotWhitespace => Ast::CharClass(CharClass::new(
                true,
                vec![CharClassItem::Named(NamedClass::Whitespace)],
            )),
            NamedClassToken::WordBoundary => Ast::Anchor(Anchor::WordBoundary),
            NamedClassToken::NotWordBoundary => Ast::Anchor(Anchor::NotWordBoundary),
        }
    }

    /// Parse a capture group: (expr)
    /// Parse an optional mrab-style fuzziness specifier following a
    /// backreference, e.g. the `{e<=1}` in `\1{e<=1}` or `\k<n>{s<=1}`.
    /// Returns `Fuzziness::Inherited` when none is present (or the `{...}` is a
    /// plain repetition, which the caller/quantifier layer handles separately).
    fn parse_optional_backref_fuzziness(&mut self) -> Result<Fuzziness> {
        if matches!(self.lexer.peek_token()?, Token::OpenBrace) && self.peek_is_mrab_fuzziness() {
            self.lexer.next_token()?; // consume '{'
            self.parse_mrab_fuzziness()
        } else {
            Ok(Fuzziness::Inherited)
        }
    }

    fn parse_capture_group(&mut self) -> Result<Ast> {
        self.capture_count += 1;
        let index = self.capture_count;

        let expr = self.parse_alternation()?;

        match self.lexer.next_token()? {
            Token::CloseParen => {}
            _ => return Err(Error::unclosed("group", self.lexer.position())),
        }

        // Check for fuzziness on the group - supports both `~N` and mrab-style
        // `{i<=1}`/`{e<=1}` (mirrors the named-capture and non-capturing paths).
        let fuzziness = match self.lexer.peek_token()? {
            Token::Tilde => {
                self.lexer.next_token()?;
                self.parse_fuzziness()?
            }
            Token::OpenBrace => {
                // Distinguish mrab fuzziness `{e<=1}` from a repetition `{1,2}`.
                if self.peek_is_mrab_fuzziness() {
                    self.lexer.next_token()?; // consume '{'
                    self.parse_mrab_fuzziness()?
                } else {
                    Fuzziness::Inherited
                }
            }
            _ => Fuzziness::Inherited,
        };

        // If there's fuzziness on a capture group, wrap the content
        let expr = if matches!(fuzziness, Fuzziness::Inherited) {
            expr
        } else {
            Self::apply_group_fuzziness(expr, fuzziness)
        };

        Ok(Ast::Group {
            index,
            name: None,
            expr: Box::new(expr),
        })
    }

    /// Parse a named capture group: (?<name>expr) or (?P<name>expr)
    /// Supports both `(?P<word>foo)~2` and mrab-style `(?P<word>foo){i<=1,d<=2}`
    fn parse_named_capture_group(&mut self, name: String) -> Result<Ast> {
        self.capture_count += 1;
        let index = self.capture_count;
        // Register the name before parsing the body so a backreference inside
        // the group can refer to an already-opened outer name, matching the
        // behaviour of numeric groups.
        self.capture_names.insert(name.clone(), index);

        let expr = self.parse_alternation()?;

        match self.lexer.next_token()? {
            Token::CloseParen => {}
            _ => return Err(Error::unclosed("named group", self.lexer.position())),
        }

        // Check for fuzziness on the group - supports both ~ and mrab-style {i<=1}
        let fuzziness = match self.lexer.peek_token()? {
            Token::Tilde => {
                self.lexer.next_token()?;
                self.parse_fuzziness()?
            }
            Token::OpenBrace => {
                // Check if this looks like mrab-style fuzziness {i<=1} vs quantifier {1,2}
                if self.peek_is_mrab_fuzziness() {
                    self.lexer.next_token()?; // consume '{'
                    self.parse_mrab_fuzziness()?
                } else {
                    Fuzziness::Inherited
                }
            }
            _ => Fuzziness::Inherited,
        };

        let expr = if matches!(fuzziness, Fuzziness::Inherited) {
            expr
        } else {
            Self::apply_group_fuzziness(expr, fuzziness)
        };

        Ok(Ast::Group {
            index,
            name: Some(name),
            expr: Box::new(expr),
        })
    }

    /// Parse a non-capturing group: (?:expr)
    /// Supports both `(?:expr)~2` and mrab-style `(?:expr){i<=1,d<=2}`
    fn parse_non_capturing_group(&mut self) -> Result<Ast> {
        let expr = self.parse_alternation()?;

        match self.lexer.next_token()? {
            Token::CloseParen => {}
            _ => {
                return Err(Error::unclosed(
                    "non-capturing group",
                    self.lexer.position(),
                ));
            }
        }

        // Check for fuzziness on the group - supports both ~ and mrab-style {i<=1}
        let fuzziness = match self.lexer.peek_token()? {
            Token::Tilde => {
                self.lexer.next_token()?;
                self.parse_fuzziness()?
            }
            Token::OpenBrace => {
                // Check if this looks like mrab-style fuzziness {i<=1} vs quantifier {1,2}
                if self.peek_is_mrab_fuzziness() {
                    self.lexer.next_token()?; // consume '{'
                    self.parse_mrab_fuzziness()?
                } else {
                    Fuzziness::Inherited
                }
            }
            _ => Fuzziness::Inherited,
        };

        Ok(Ast::NonCapturingGroup {
            expr: Box::new(expr),
            fuzziness,
        })
    }

    /// Parse an atomic group: (?>expr)
    /// Once the group matches, backtracking is disabled within the group.
    fn parse_atomic_group(&mut self) -> Result<Ast> {
        let expr = self.parse_alternation()?;

        match self.lexer.next_token()? {
            Token::CloseParen => {}
            _ => return Err(Error::unclosed("atomic group", self.lexer.position())),
        }

        Ok(Ast::AtomicGroup {
            expr: Box::new(expr),
        })
    }

    /// Check if the next brace starts mrab-style fuzziness (not a quantifier).
    /// mrab-style starts with a letter (i, d, s, e, t, c) or a digit followed by a letter.
    fn peek_is_mrab_fuzziness(&self) -> bool {
        // Save state
        let saved_chars = self.lexer.remaining();

        // After '{', check first character
        if let Some(first_char) = saved_chars.chars().nth(1) {
            // mrab-style: {i<=1} or {1<=e<=3} or {2i+2d+1s<=4} or {c<=4}
            // quantifier: {1,2} or {3}
            match first_char {
                'i' | 'd' | 's' | 'e' | 't' | 'c' => return true,
                c if c.is_ascii_digit() => {
                    // Could be quantifier {1,2} or cost constraint {2i+1d<=3}
                    // Look for a letter after the digits
                    let rest = &saved_chars[1..];
                    for ch in rest.chars() {
                        if ch == ',' || ch == '}' {
                            return false; // It's a quantifier
                        }
                        if ch == '<' || ch == '=' {
                            return true; // It's mrab-style
                        }
                        if ch.is_alphabetic() {
                            return true; // {2i...} is cost constraint
                        }
                        if !ch.is_ascii_digit() {
                            break;
                        }
                    }
                }
                _ => return false,
            }
        }
        false
    }

    /// Parse mrab-style fuzziness: {i<=1,d<=2,s<=3,t<=1} or {e<=5} or {2i+2d+1s+1t<=4} or {c<=4}
    fn parse_mrab_fuzziness(&mut self) -> Result<Fuzziness> {
        let start_pos = self.lexer.position();
        let mut mrab = MrabFuzziness::new();

        loop {
            match self.lexer.peek_token()? {
                Token::CloseBrace => {
                    self.lexer.next_token()?;
                    break;
                }
                Token::Char(c) if c.is_ascii_digit() => {
                    // Cost constraint: 2i+2d+1s<=4 or range: 1<=e<=3 or 1<e<3
                    let num = self.parse_number_u8()?;

                    match self.lexer.peek_token()? {
                        Token::Char('<') => {
                            // Range: 1<=e<=3 or 1<e<3
                            self.lexer.next_token()?; // '<'
                            let lower_inclusive =
                                matches!(self.lexer.peek_token()?, Token::Char('='));
                            if lower_inclusive {
                                self.lexer.next_token()?; // '='
                            }

                            // Get the error type
                            let Token::Char(key) = self.lexer.next_token()? else {
                                return Err(Error::invalid_fuzziness(
                                    start_pos,
                                    "expected error type",
                                ));
                            };

                            if key != 'e' {
                                return Err(Error::invalid_fuzziness(
                                    start_pos,
                                    "range constraint only valid for 'e'",
                                ));
                            }

                            // Exclusive lower bound: 1<e means min_errors = 2
                            let min_val = if lower_inclusive { num } else { num + 1 };
                            mrab.min_errors = Some(min_val);

                            // Check for upper bound
                            if matches!(self.lexer.peek_token()?, Token::Char('<')) {
                                self.lexer.next_token()?;
                                let upper_inclusive =
                                    matches!(self.lexer.peek_token()?, Token::Char('='));
                                if upper_inclusive {
                                    self.lexer.next_token()?;
                                }
                                let mut max = self.parse_number_u8()?;
                                // Exclusive upper bound: e<3 means max_errors = 2
                                if !upper_inclusive && max > 0 {
                                    max -= 1;
                                }
                                mrab.max_errors = Some(max);
                            }
                        }
                        Token::Char(k) if "idst".contains(k) => {
                            // Cost constraint: 2i+2d+1s+1t<=4
                            self.lexer.next_token()?;
                            match k {
                                'i' => mrab.insertion_cost = Some(num),
                                'd' => mrab.deletion_cost = Some(num),
                                's' => mrab.substitution_cost = Some(num),
                                't' => mrab.transposition_cost = Some(num),
                                _ => {}
                            }
                            // Continue parsing cost expression
                            self.parse_cost_continuation(&mut mrab, start_pos)?;
                        }
                        _ => {
                            return Err(Error::invalid_fuzziness(
                                start_pos,
                                "expected error type or '<'",
                            ));
                        }
                    }
                }
                Token::Char(key) if "idsetc".contains(key) => {
                    self.lexer.next_token()?;

                    // Check for <=, <, or just allowed type
                    match self.lexer.peek_token()? {
                        Token::Char('<') => {
                            self.lexer.next_token()?;
                            // Check for <= (inclusive) or just < (exclusive)
                            let inclusive = matches!(self.lexer.peek_token()?, Token::Char('='));
                            if inclusive {
                                self.lexer.next_token()?;
                            }
                            let mut value = self.parse_number_u8()?;

                            // Exclusive bound: {i<3} means at most 2
                            if !inclusive && value > 0 {
                                value -= 1;
                            }

                            match key {
                                'i' => mrab.max_insertions = Some(value),
                                'd' => mrab.max_deletions = Some(value),
                                's' => mrab.max_substitutions = Some(value),
                                't' => mrab.max_transpositions = Some(value),
                                'e' => mrab.max_errors = Some(value),
                                'c' => {
                                    // Simple cost syntax: {c<=4} means {1i+1d+1s+1t<=4}
                                    // For <=N, we store N+1 so that `cost < N+1` is equivalent to `cost <= N`
                                    let adjusted = if inclusive { value + 1 } else { value };
                                    mrab.insertion_cost = Some(1);
                                    mrab.deletion_cost = Some(1);
                                    mrab.substitution_cost = Some(1);
                                    mrab.transposition_cost = Some(1);
                                    mrab.max_cost = Some(adjusted);
                                }
                                _ => {}
                            }
                        }
                        Token::Char(',' | ':') | Token::CloseBrace => {
                            // Just the type allowed (unlimited); `:` introduces a
                            // char restriction on that unlimited op, e.g. `{i:[ ]}`
                            // (mrab Hg issue 442). The ':' is left for the
                            // restriction handler below.
                            // {i} means insertions allowed (unlimited)
                            match key {
                                'i' => mrab.unlimited_insertions = true,
                                'd' => mrab.unlimited_deletions = true,
                                's' => mrab.unlimited_substitutions = true,
                                't' => mrab.unlimited_transpositions = true,
                                'e' => mrab.unlimited_errors = true,
                                'c' => {
                                    // {c} without value doesn't make sense, treat as unlimited errors
                                    mrab.unlimited_errors = true;
                                }
                                _ => {}
                            }
                        }
                        _ => {
                            return Err(Error::invalid_fuzziness(
                                start_pos,
                                "expected '<', '<=' or ','",
                            ));
                        }
                    }
                }
                _ => {
                    return Err(Error::invalid_fuzziness(
                        start_pos,
                        "invalid fuzziness specification",
                    ));
                }
            }

            // Check for comma separator or character class restriction
            match self.lexer.peek_token()? {
                Token::Char(',') => {
                    self.lexer.next_token()?;
                }
                Token::Char(':') => {
                    // Character class restriction: {s<=2:[a-z]} or {s<=2:\d}
                    self.lexer.next_token()?; // consume ':'
                    let class = self.parse_char_class_restriction()?;
                    // Apply to substitutions by default, or to all types
                    // mrab-regex applies to all error types, we'll store it for substitutions
                    mrab.substitution_chars = Some(class.clone());
                    mrab.insertion_chars = Some(class.clone());
                    mrab.deletion_chars = Some(class);
                }
                _ => {}
            }
        }

        Ok(Fuzziness::MrabStyle(mrab))
    }

    /// Parse a character class restriction in mrab-style fuzziness: [a-z] or \d
    fn parse_char_class_restriction(&mut self) -> Result<CharClass> {
        match self.lexer.peek_token()? {
            Token::OpenBracket => {
                self.lexer.next_token()?; // consume '['
                self.parse_char_class_inner()
            }
            Token::NamedClass(class) => {
                self.lexer.next_token()?;
                // Convert named class to CharClass
                let items = match class {
                    NamedClassToken::Digit => vec![CharClassItem::Named(NamedClass::Digit)],
                    NamedClassToken::NotDigit => vec![CharClassItem::Named(NamedClass::NotDigit)],
                    NamedClassToken::Word => vec![CharClassItem::Named(NamedClass::Word)],
                    NamedClassToken::NotWord => vec![CharClassItem::Named(NamedClass::NotWord)],
                    NamedClassToken::Whitespace => {
                        vec![CharClassItem::Named(NamedClass::Whitespace)]
                    }
                    NamedClassToken::NotWhitespace => {
                        vec![CharClassItem::Named(NamedClass::NotWhitespace)]
                    }
                    _ => {
                        return Err(Error::invalid_fuzziness(
                            self.lexer.position(),
                            "invalid character class in fuzziness restriction",
                        ));
                    }
                };
                Ok(CharClass::new(false, items))
            }
            // Bare single character, e.g. `{e<=1:u}` (mrab Hg issue 338).
            // Equivalent to the bracketed form `{e<=1:[u]}`.
            Token::Char(c) if c != ',' && c != '}' => {
                self.lexer.next_token()?;
                Ok(CharClass::new(false, vec![CharClassItem::Single(c)]))
            }
            _ => Err(Error::invalid_fuzziness(
                self.lexer.position(),
                "expected character class after ':'",
            )),
        }
    }

    /// Parse the inner contents of a character class (after '[' is consumed).
    fn parse_char_class_inner(&mut self) -> Result<CharClass> {
        let start_pos = self.lexer.position();
        let mut items = Vec::new();

        // Check for negation
        let negated = matches!(self.lexer.peek_token()?, Token::Caret);
        if negated {
            self.lexer.next_token()?;
        }

        loop {
            match self.lexer.peek_token()? {
                Token::CloseBracket => {
                    self.lexer.next_token()?;
                    break;
                }
                Token::Eof => return Err(Error::unclosed("character class", start_pos)),
                _ => {
                    let new_items = self.parse_char_class_item()?;
                    items.extend(new_items);
                }
            }
        }

        Ok(CharClass::new(negated, items))
    }

    /// Parse the rest of a cost constraint after the first term.
    fn parse_cost_continuation(
        &mut self,
        mrab: &mut MrabFuzziness,
        start_pos: usize,
    ) -> Result<()> {
        loop {
            match self.lexer.peek_token()? {
                Token::Plus | Token::Char('+') => {
                    self.lexer.next_token()?;
                    let cost = self.parse_number_u8()?;
                    let key = match self.lexer.next_token()? {
                        Token::Char(k) if "idst".contains(k) => k,
                        _ => {
                            return Err(Error::invalid_fuzziness(
                                start_pos,
                                "expected error type after cost",
                            ));
                        }
                    };
                    match key {
                        'i' => mrab.insertion_cost = Some(cost),
                        'd' => mrab.deletion_cost = Some(cost),
                        's' => mrab.substitution_cost = Some(cost),
                        't' => mrab.transposition_cost = Some(cost),
                        _ => {}
                    }
                }
                Token::Char('<') => {
                    self.lexer.next_token()?;
                    // Check for <= (inclusive) or just < (exclusive)
                    let inclusive = matches!(self.lexer.peek_token()?, Token::Char('='));
                    if inclusive {
                        self.lexer.next_token()?;
                    }
                    let max_cost = self.parse_number_u8()?;
                    // Note: We store the value as-is. The is_satisfied check uses `<` for
                    // exclusive bounds and `<=` would need special handling. Since we always
                    // use `<` in is_satisfied, we don't need to adjust the value here.
                    // For `1i+1d<2`, max_cost=2 and check is `cost < 2`.
                    // For `1i+1d<=2`, we'd need max_cost=3 to make `cost < 3` equivalent to `<=2`.
                    let adjusted_cost = if inclusive { max_cost + 1 } else { max_cost };
                    mrab.max_cost = Some(adjusted_cost);
                    break;
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// Apply fuzziness to all literals in a group.
    fn apply_group_fuzziness(ast: Ast, fuzziness: Fuzziness) -> Ast {
        match ast {
            Ast::Literal { text, .. } => Ast::Literal {
                text,
                fuzziness: fuzziness.clone(),
            },
            Ast::Char(ch) => Ast::Literal {
                text: ch.to_string(),
                fuzziness,
            },
            Ast::Concat(parts) => Ast::Concat(
                parts
                    .into_iter()
                    .map(|p| Self::apply_group_fuzziness(p, fuzziness.clone()))
                    .collect(),
            ),
            Ast::Alternation(alts) => Ast::Alternation(
                alts.into_iter()
                    .map(|a| Self::apply_group_fuzziness(a, fuzziness.clone()))
                    .collect(),
            ),
            Ast::Group { index, name, expr } => Ast::Group {
                index,
                name,
                expr: Box::new(Self::apply_group_fuzziness(*expr, fuzziness)),
            },
            Ast::NonCapturingGroup { expr, .. } => Ast::NonCapturingGroup {
                expr: Box::new(Self::apply_group_fuzziness(*expr, fuzziness.clone())),
                fuzziness,
            },
            Ast::Quantified {
                expr,
                quantifier,
                greedy,
            } => Ast::Quantified {
                expr: Box::new(Self::apply_group_fuzziness(*expr, fuzziness)),
                quantifier,
                greedy,
            },
            // Other nodes pass through unchanged
            other => other,
        }
    }

    /// Parse a lookahead: (?=expr) or (?!expr)
    fn parse_lookahead(&mut self, positive: bool) -> Result<Ast> {
        let expr = self.parse_alternation()?;

        match self.lexer.next_token()? {
            Token::CloseParen => {}
            _ => return Err(Error::unclosed("lookahead", self.lexer.position())),
        }

        Ok(Ast::Lookahead {
            positive,
            expr: Box::new(expr),
        })
    }

    /// Parse a lookbehind: (?<=expr) or (?<!expr)
    fn parse_lookbehind(&mut self, positive: bool) -> Result<Ast> {
        let expr = self.parse_alternation()?;

        match self.lexer.next_token()? {
            Token::CloseParen => {}
            _ => return Err(Error::unclosed("lookbehind", self.lexer.position())),
        }

        Ok(Ast::Lookbehind {
            positive,
            expr: Box::new(expr),
        })
    }

    /// Parse a character class: [...]
    fn parse_char_class(&mut self) -> Result<Ast> {
        let start_pos = self.lexer.position();
        let mut items = Vec::new();

        // Check for negation
        let negated = matches!(self.lexer.peek_token()?, Token::Caret);
        if negated {
            self.lexer.next_token()?;
        }

        // First character can be ] or - literally
        if matches!(self.lexer.peek_token()?, Token::CloseBracket) {
            self.lexer.next_token()?;
            items.push(CharClassItem::Single(']'));
        }

        loop {
            match self.lexer.peek_token()? {
                Token::CloseBracket => {
                    self.lexer.next_token()?;
                    break;
                }
                Token::Eof => return Err(Error::unclosed("character class", start_pos)),
                _ => {
                    let new_items = self.parse_char_class_item()?;
                    items.extend(new_items);
                }
            }
        }

        if items.is_empty() {
            return Err(Error::invalid_char_class(
                start_pos,
                "empty character class",
            ));
        }

        Ok(Ast::CharClass(CharClass::new(negated, items)))
    }

    /// Parse one or more items in a character class.
    /// Handles named classes (\d, \w, \s, etc.) as well as single chars and ranges.
    /// Returns a Vec because trailing hyphen case needs to return two items (char + hyphen).
    fn parse_char_class_item(&mut self) -> Result<Vec<CharClassItem>> {
        // Check for named class first
        if let Token::NamedClass(class) = self.lexer.peek_token()? {
            self.lexer.next_token()?; // consume the named class token
            let named = match class {
                NamedClassToken::Digit => NamedClass::Digit,
                NamedClassToken::NotDigit => NamedClass::NotDigit,
                NamedClassToken::Word => NamedClass::Word,
                NamedClassToken::NotWord => NamedClass::NotWord,
                NamedClassToken::Whitespace => NamedClass::Whitespace,
                NamedClassToken::NotWhitespace => NamedClass::NotWhitespace,
                // Word boundaries don't make sense inside character classes
                NamedClassToken::WordBoundary | NamedClassToken::NotWordBoundary => {
                    return Err(Error::invalid_char_class(
                        self.lexer.position(),
                        "word boundary \\b/\\B not valid inside character class".to_string(),
                    ));
                }
            };
            return Ok(vec![CharClassItem::Named(named)]);
        }

        let ch = self.parse_char_class_char()?;

        // Check for range
        if matches!(self.lexer.peek_token()?, Token::Hyphen) {
            // Peek ahead to see if this is a range or literal hyphen
            let saved_pos = self.lexer.position();
            self.lexer.next_token()?; // consume '-'

            if self.lexer.peek_token()? == Token::CloseBracket {
                // Hyphen at end is literal - return both the char and the hyphen
                return Ok(vec![CharClassItem::Single(ch), CharClassItem::Single('-')]);
            } else {
                let end_ch = self.parse_char_class_char()?;
                if end_ch < ch {
                    return Err(Error::invalid_char_class(
                        saved_pos,
                        format!("invalid range: '{ch}'-'{end_ch}' (end before start)"),
                    ));
                }
                return Ok(vec![CharClassItem::Range(ch, end_ch)]);
            }
        }

        Ok(vec![CharClassItem::Single(ch)])
    }

    /// Parse a single character in a character class.
    /// Inside character classes, most metacharacters are treated as literals.
    fn parse_char_class_char(&mut self) -> Result<char> {
        match self.lexer.next_token()? {
            Token::Char(ch) | Token::Escaped(ch) => Ok(ch),
            Token::Hyphen => Ok('-'),
            // Metacharacters are literal inside character classes
            Token::Dot => Ok('.'),
            Token::Star => Ok('*'),
            Token::Plus => Ok('+'),
            Token::Question => Ok('?'),
            Token::Pipe => Ok('|'),
            Token::Caret => Ok('^'), // Only special at start, but handled separately
            Token::Dollar => Ok('$'),
            Token::OpenParen => Ok('('),
            Token::CloseParen => Ok(')'),
            Token::OpenBrace => Ok('{'),
            Token::CloseBrace => Ok('}'),
            Token::Tilde => Ok('~'),
            Token::NamedClass(class) => {
                // Named classes in character classes - expand them
                // For now, return a placeholder - this should be handled specially
                Err(Error::invalid_char_class(
                    self.lexer.position(),
                    format!(
                        "named class {class:?} not supported inside character class (use separately)"
                    ),
                ))
            }
            token => Err(Error::invalid_char_class(
                self.lexer.position(),
                format!("unexpected token in character class: {token:?}"),
            )),
        }
    }
}

/// Result of parsing a pattern, including both AST and flags.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// The parsed AST.
    pub ast: Ast,
    /// Match flags found in the pattern.
    pub flags: MatchFlags,
    /// Number of capture groups.
    pub capture_count: usize,
}

/// Parse a pattern into an AST.
pub fn parse(pattern: &str) -> Result<Ast> {
    let parser = Parser::new(pattern);
    parser.parse()
}

/// Parse a pattern into an AST with flags and metadata.
pub fn parse_with_flags(
    pattern: &str,
    verbose: bool,
    dot_all: bool,
    ungreedy: bool,
) -> Result<ParseResult> {
    let mut parser = Parser::new_with_flags(pattern, verbose, dot_all, ungreedy);
    // Parse the pattern - this parses alternation and accumulates flags
    let ast = parser.parse_alternation()?;

    if !matches!(parser.lexer.peek_token()?, Token::Eof) {
        return Err(Error::parse(
            parser.lexer.position(),
            "unexpected token after pattern",
        ));
    }

    Ok(ParseResult {
        ast,
        flags: parser.flags,
        capture_count: parser.capture_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_literal() {
        let ast = parse("hello").unwrap();
        assert!(matches!(ast, Ast::Literal { text, .. } if text == "hello"));
    }

    #[test]
    fn test_alternation() {
        let ast = parse("a|b|c").unwrap();
        assert!(matches!(ast, Ast::Alternation(_)));
    }

    #[test]
    fn test_quantifiers() {
        let ast = parse("a*b+c?d{2}e{1,3}").unwrap();
        assert!(matches!(ast, Ast::Concat(_)));
    }

    #[test]
    fn test_capture_group() {
        let ast = parse("(abc)").unwrap();
        assert!(matches!(ast, Ast::Group { index: 1, .. }));
    }

    #[test]
    fn test_char_class() {
        let ast = parse("[a-z]").unwrap();
        assert!(matches!(ast, Ast::CharClass(_)));
    }

    #[test]
    fn test_fuzziness_simple() {
        let ast = parse("hello~2").unwrap();
        match ast {
            Ast::Literal { text, fuzziness } => {
                assert_eq!(text, "hello");
                assert!(matches!(fuzziness, Fuzziness::Edits(2)));
            }
            _ => panic!("expected literal with fuzziness"),
        }
    }

    #[test]
    fn test_fuzziness_detailed() {
        let ast = parse("hello~{i=1,d=2}").unwrap();
        match ast {
            Ast::Literal { text, fuzziness } => {
                assert_eq!(text, "hello");
                assert!(matches!(fuzziness, Fuzziness::Detailed(_)));
            }
            _ => panic!("expected literal with detailed fuzziness"),
        }
    }

    #[test]
    fn test_group_fuzziness() {
        let ast = parse("(hello world)~2").unwrap();
        assert!(matches!(ast, Ast::Group { .. }));
    }

    #[test]
    fn test_lookahead() {
        let ast = parse("(?=abc)").unwrap();
        assert!(matches!(ast, Ast::Lookahead { positive: true, .. }));

        let ast = parse("(?!abc)").unwrap();
        assert!(matches!(
            ast,
            Ast::Lookahead {
                positive: false,
                ..
            }
        ));
    }

    #[test]
    fn test_anchors() {
        let ast = parse("^hello$").unwrap();
        match ast {
            Ast::Concat(parts) => {
                assert!(matches!(parts[0], Ast::Anchor(Anchor::Start)));
                assert!(matches!(parts[2], Ast::Anchor(Anchor::End)));
            }
            _ => panic!("expected concat"),
        }
    }

    #[test]
    fn test_named_group() {
        let ast = parse("(?<name>abc)").unwrap();
        match ast {
            Ast::Group { name, .. } => {
                assert_eq!(name, Some("name".to_string()));
            }
            _ => panic!("expected named group"),
        }
    }

    #[test]
    fn test_complex_pattern() {
        // Test with char class (no fuzziness - fuzziness only applies to literals)
        let ast = parse(r"^(\d{3})-(\w+)$").unwrap();
        assert!(matches!(ast, Ast::Concat(_)));

        // Test with fuzzy literal in a group
        let ast = parse(r"^(\d{3})-(test~2)$").unwrap();
        assert!(matches!(ast, Ast::Concat(_)));
    }

    // ==================== mrab-regex syntax tests ====================

    #[test]
    fn test_mrab_basic_limits() {
        // {i<=1} - max 1 insertion
        let ast = parse("(?:hello){i<=1}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                assert!(matches!(fuzziness, Fuzziness::MrabStyle(_)));
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_insertions, Some(1));
                    assert_eq!(mrab.max_deletions, None);
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_combined_limits() {
        // {i<=1,d<=2,s<=3} - combined limits
        let ast = parse("(?:hello){i<=1,d<=2,s<=3}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_insertions, Some(1));
                    assert_eq!(mrab.max_deletions, Some(2));
                    assert_eq!(mrab.max_substitutions, Some(3));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_total_errors() {
        // {e<=5} - max 5 total errors
        let ast = parse("(?:hello){e<=5}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_errors, Some(5));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_error_range() {
        // {1<=e<=3} - between 1 and 3 errors
        let ast = parse("(?:hello){1<=e<=3}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.min_errors, Some(1));
                    assert_eq!(mrab.max_errors, Some(3));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_exclusive_bounds() {
        // {i<3} - fewer than 3 insertions (i.e., at most 2)
        let ast = parse("(?:hello){i<3}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_insertions, Some(2)); // <3 means <=2
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_exclusive_range() {
        // {1<e<5} - more than 1 and fewer than 5 errors (i.e., 2, 3, or 4)
        let ast = parse("(?:hello){1<e<5}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.min_errors, Some(2)); // >1 means >=2
                    assert_eq!(mrab.max_errors, Some(4)); // <5 means <=4
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_cost_constraint() {
        // {2i+2d+1s<=4} - weighted costs
        // For <=N, we store N+1 so that `cost < N+1` is equivalent to `cost <= N`
        let ast = parse("(?:hello){2i+2d+1s<=4}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.insertion_cost, Some(2));
                    assert_eq!(mrab.deletion_cost, Some(2));
                    assert_eq!(mrab.substitution_cost, Some(1));
                    // max_cost = 5 because <=4 is stored as <5
                    assert_eq!(mrab.max_cost, Some(5));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_char_class_restriction() {
        // {s<=2:[a-z]} - substitutions from [a-z]
        let ast = parse("(?:hello){s<=2:[a-z]}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_substitutions, Some(2));
                    assert!(mrab.substitution_chars.is_some());
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_char_class_restriction_escape() {
        // {i<=3:\d} - insertions must be digits
        let ast = parse(r"(?:hello){i<=3:\d}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_insertions, Some(3));
                    assert!(mrab.insertion_chars.is_some());
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_bestmatch_flag() {
        // (?b) - BESTMATCH flag
        let result = parse_with_flags("(?b)hello", false, false, false).unwrap();
        assert!(result.flags.best_match);
        assert!(!result.flags.enhance_match);
    }

    #[test]
    fn test_enhancematch_flag() {
        // (?e) - ENHANCEMATCH flag
        let result = parse_with_flags("(?e)hello", false, false, false).unwrap();
        assert!(!result.flags.best_match);
        assert!(result.flags.enhance_match);
    }

    #[test]
    fn test_combined_flags() {
        // Both flags together
        let result = parse_with_flags("(?b)(?e)hello", false, false, false).unwrap();
        assert!(result.flags.best_match);
        assert!(result.flags.enhance_match);
    }

    #[test]
    fn test_mrab_just_type_allowed() {
        // {i} - insertions allowed (unlimited)
        let ast = parse("(?:hello){i}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    // When just the type is specified, no limit is set
                    assert_eq!(mrab.max_insertions, None);
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_mixed_constraints() {
        // {i<=2,d<=2,e<=3} - individual limits with total limit
        let ast = parse("(?:hello){i<=2,d<=2,e<=3}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_insertions, Some(2));
                    assert_eq!(mrab.max_deletions, Some(2));
                    assert_eq!(mrab.max_errors, Some(3));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    // ==================== transposition (t) syntax tests ====================

    #[test]
    fn test_mrab_transposition_limit() {
        // {t<=2} - max 2 transpositions
        let ast = parse("(?:hello){t<=2}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_transpositions, Some(2));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_transposition_exclusive() {
        // {t<3} - fewer than 3 transpositions (i.e., at most 2)
        let ast = parse("(?:hello){t<3}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_transpositions, Some(2)); // <3 means <=2
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_transposition_combined() {
        // {i<=1,d<=1,s<=1,t<=1} - all edit types with limits
        let ast = parse("(?:hello){i<=1,d<=1,s<=1,t<=1}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.max_insertions, Some(1));
                    assert_eq!(mrab.max_deletions, Some(1));
                    assert_eq!(mrab.max_substitutions, Some(1));
                    assert_eq!(mrab.max_transpositions, Some(1));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_transposition_unlimited() {
        // {t} - transpositions allowed (unlimited)
        let ast = parse("(?:hello){t}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert!(mrab.unlimited_transpositions);
                    assert_eq!(mrab.max_transpositions, None);
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_transposition_cost() {
        // {2i+2d+1s+1t<=4} - cost constraint with transposition
        let ast = parse("(?:hello){2i+2d+1s+1t<=4}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.insertion_cost, Some(2));
                    assert_eq!(mrab.deletion_cost, Some(2));
                    assert_eq!(mrab.substitution_cost, Some(1));
                    assert_eq!(mrab.transposition_cost, Some(1));
                    // max_cost = 5 because <=4 is stored as <5
                    assert_eq!(mrab.max_cost, Some(5));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    // ==================== simple cost (c<=N) syntax tests ====================

    #[test]
    fn test_mrab_simple_cost() {
        // {c<=4} - simple cost syntax (equal weights for all edit types)
        let ast = parse("(?:hello){c<=4}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.insertion_cost, Some(1));
                    assert_eq!(mrab.deletion_cost, Some(1));
                    assert_eq!(mrab.substitution_cost, Some(1));
                    assert_eq!(mrab.transposition_cost, Some(1));
                    // max_cost = 5 because <=4 is stored as <5
                    assert_eq!(mrab.max_cost, Some(5));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_simple_cost_exclusive() {
        // {c<3} - simple cost with exclusive bound
        let ast = parse("(?:hello){c<3}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    assert_eq!(mrab.insertion_cost, Some(1));
                    assert_eq!(mrab.deletion_cost, Some(1));
                    assert_eq!(mrab.substitution_cost, Some(1));
                    assert_eq!(mrab.transposition_cost, Some(1));
                    // max_cost = 2 because <3 means cost < 3 (i.e., cost <= 2)
                    assert_eq!(mrab.max_cost, Some(2));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }

    #[test]
    fn test_mrab_to_limits_transposition() {
        // Verify to_limits() correctly converts transposition
        let ast = parse("(?:hello){t<=2}").unwrap();
        match ast {
            Ast::NonCapturingGroup { fuzziness, .. } => {
                if let Fuzziness::MrabStyle(mrab) = fuzziness {
                    let limits = mrab.to_limits();
                    assert_eq!(limits.get_swaps(), Some(2));
                } else {
                    panic!("expected MrabStyle fuzziness");
                }
            }
            _ => panic!("expected non-capturing group"),
        }
    }
}
