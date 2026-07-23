//! Lexer for tokenizing fuzzy regex patterns.

use crate::error::{Error, Result};

/// A set of inline flags parsed from a group like `(?ims)` / `(?es)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InlineFlags {
    /// `b` - BESTMATCH.
    pub best_match: bool,
    /// `e` - ENHANCEMATCH.
    pub enhance_match: bool,
    /// `p` - POSIX leftmost-longest.
    pub posix: bool,
    /// `x` - verbose.
    pub verbose: bool,
    /// `s` - dot-all.
    pub dot_all: bool,
    /// `m` - multi-line.
    pub multi_line: bool,
    /// `U` - ungreedy.
    pub ungreedy: bool,
    /// `i` - case-insensitive.
    pub case_insensitive: bool,
    /// `g` - global.
    pub global: bool,
    /// `u` - unicode.
    pub unicode: bool,
    /// `r` - reverse (search from the end of the text toward the start).
    pub reverse: bool,
}

/// Token produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Literal character.
    Char(char),
    /// Escaped character (e.g., `\n`, `\t`).
    Escaped(char),
    /// Named class escape (e.g., `\d`, `\w`, `\s`).
    NamedClass(NamedClassToken),
    /// Named list reference `\L<name>`.
    NamedList(String),
    /// Backreference `\1`, `\2`, etc.
    Backreference(usize),
    /// Named backreference `\k<name>`, `\k{name}`, or `(?P=name)`.
    NamedBackreference(String),
    /// `(` - open group.
    OpenParen,
    /// `)` - close group.
    CloseParen,
    /// `[` - open character class.
    OpenBracket,
    /// `]` - close character class.
    CloseBracket,
    /// `{` - open quantifier.
    OpenBrace,
    /// `}` - close quantifier.
    CloseBrace,
    /// `|` - alternation.
    Pipe,
    /// `^` - start anchor or negation in char class.
    Caret,
    /// `$` - end anchor.
    Dollar,
    /// `.` - any character.
    Dot,
    /// `*` - zero or more.
    Star,
    /// `+` - one or more.
    Plus,
    /// `?` - zero or one, or non-greedy modifier.
    Question,
    /// `*+` - possessive zero or more (only after atom, not after another quantifier).
    StarPossessive,
    /// `++` - possessive one or more (only after atom).
    PlusPossessive,
    /// `?+` - possessive zero or one (only after atom).
    QuestionPossessive,
    /// `~` - fuzziness marker.
    Tilde,
    /// `-` - range in character class.
    Hyphen,
    /// `(?:` - non-capturing group.
    NonCapturing,
    /// `(?=` - positive lookahead.
    PositiveLookahead,
    /// `(?!` - negative lookahead.
    NegativeLookahead,
    /// `(?<=` - positive lookbehind.
    PositiveLookbehind,
    /// `(?<!` - negative lookbehind.
    NegativeLookbehind,
    /// `(?P<name>` or `(?<name>` - named group.
    NamedGroup(String),
    /// `(?b)` - BESTMATCH flag (search for best match instead of first).
    BestMatch,
    /// `(?e)` - ENHANCEMATCH flag (improve fit of fuzzy match).
    EnhanceMatch,
    /// `(?p)` - POSIX leftmost-longest matching.
    PosixMatch,
    /// `(?x)` - Verbose mode (ignore whitespace, allow comments).
    Verbose,
    /// `(?s)` - Dot-all mode (`.` matches newlines).
    DotAll,
    /// `(?m)` - Multi-line mode (`^`/`$` match at line boundaries).
    MultiLine,
    /// `(?U)` - Ungreedy mode (invert default greediness).
    Ungreedy,
    /// `(?i)` - Case-insensitive mode.
    CaseInsensitive,
    /// `(?g)` - Global mode (find all matches).
    Global,
    /// `(?u)` - Unicode mode (enable Unicode character classes).
    Unicode,
    /// A run of inline flags, e.g. `(?es)` or `(?im)`. Each `true` field sets
    /// the corresponding flag. Single-flag groups like `(?i)` also use this.
    InlineFlags(InlineFlags),
    /// `\K` - Reset match start (keep everything before it out of the match).
    ResetMatchStart,
    /// `(?>` - Atomic group (prevent backtracking).
    AtomicGroup,
    /// `(?R)` - Recursive entire pattern.
    RecursivePattern,
    /// `(?1)`, `(?2)`, etc. - Recursive numbered group.
    RecursiveGroup(usize),
    /// `(?&name)` - Recursive named group.
    RecursiveNamedGroup(String),
    /// `(?call:name)` - Custom handler invocation.
    Handler(String),
    /// End of input.
    Eof,
}

/// Named class tokens from escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedClassToken {
    /// Digit class `\d` - matches any digit character.
    Digit,
    /// Non-digit class `\D` - matches any non-digit character.
    NotDigit,
    /// Word class `\w` - matches any word character (alphanumeric or underscore).
    Word,
    /// Non-word class `\W` - matches any non-word character.
    NotWord,
    /// Whitespace class `\s` - matches any whitespace character.
    Whitespace,
    /// Non-whitespace class `\S` - matches any non-whitespace character.
    NotWhitespace,
    /// Word boundary `\b` - matches at a word boundary position.
    WordBoundary,
    /// Non-word boundary `\B` - matches at a non-word boundary position.
    NotWordBoundary,
}

/// Lexer for regex patterns.
pub struct Lexer<'a> {
    input: &'a str,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    position: usize,
    /// Verbose mode - skip whitespace and comments.
    verbose: bool,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input.
    #[must_use]
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            chars: input.char_indices().peekable(),
            position: 0,
            verbose: false,
        }
    }

    /// Create a new lexer with verbose mode.
    #[must_use]
    pub fn new_with_flags(input: &'a str, verbose: bool) -> Self {
        Lexer {
            input,
            chars: input.char_indices().peekable(),
            position: 0,
            verbose,
        }
    }

    /// Get the current position in the input.
    #[must_use]
    pub fn position(&self) -> usize {
        self.position
    }

    /// Set the position in the input (for backtracking).
    pub fn set_position(&mut self, pos: usize) {
        self.position = pos;
        // Rebuild the chars iterator from the new position
        self.chars = self.input[pos..].char_indices().peekable();
    }

    /// Peek at the next character without consuming it.
    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().map(|(_, ch)| *ch)
    }

    /// Consume and return the next character.
    fn next_char(&mut self) -> Option<char> {
        if let Some((pos, ch)) = self.chars.next() {
            self.position = pos + ch.len_utf8();
            Some(ch)
        } else {
            None
        }
    }

    /// Try to match a specific string, advancing if successful.
    ///
    /// This is useful for parser extensions that need to match multi-character
    /// sequences like keywords or specific syntax.
    pub fn try_match(&mut self, s: &str) -> bool {
        let remaining = &self.input[self.position..];
        if remaining.starts_with(s) {
            for _ in 0..s.chars().count() {
                self.next_char();
            }
            true
        } else {
            false
        }
    }

    /// Skip whitespace and comments in verbose mode.
    fn skip_verbose_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.next_char();
            } else if ch == '#' {
                // Skip comment until end of line
                self.next_char(); // consume '#'
                while let Some(c) = self.peek_char() {
                    if c == '\n' {
                        self.next_char();
                        break;
                    }
                    self.next_char();
                }
            } else {
                break;
            }
        }
    }

    /// Get the next token.
    ///
    /// # Errors
    /// Returns an error if an invalid escape sequence is encountered.
    pub fn next_token(&mut self) -> Result<Token> {
        // In verbose mode, skip whitespace and comments
        if self.verbose {
            self.skip_verbose_whitespace();
        }

        let Some(ch) = self.next_char() else {
            return Ok(Token::Eof);
        };

        match ch {
            '(' => self.lex_group_start(),
            ')' => Ok(Token::CloseParen),
            '[' => Ok(Token::OpenBracket),
            ']' => Ok(Token::CloseBracket),
            '{' => Ok(Token::OpenBrace),
            '}' => Ok(Token::CloseBrace),
            '|' => Ok(Token::Pipe),
            '^' => Ok(Token::Caret),
            '$' => Ok(Token::Dollar),
            '.' => Ok(Token::Dot),
            '*' => Ok(Token::Star),
            '+' => Ok(Token::Plus),
            '?' => Ok(Token::Question),
            '~' => Ok(Token::Tilde),
            '-' => Ok(Token::Hyphen),
            '\\' => self.lex_escape(),
            _ => Ok(Token::Char(ch)),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn lex_group_start(&mut self) -> Result<Token> {
        // Check for special group syntax
        if self.peek_char() == Some('?') {
            self.next_char(); // consume '?'

            match self.peek_char() {
                Some(':') => {
                    self.next_char();
                    Ok(Token::NonCapturing)
                }
                Some('=') => {
                    self.next_char();
                    Ok(Token::PositiveLookahead)
                }
                Some('!') => {
                    self.next_char();
                    Ok(Token::NegativeLookahead)
                }
                Some('<') => {
                    self.next_char();
                    match self.peek_char() {
                        Some('=') => {
                            self.next_char();
                            Ok(Token::PositiveLookbehind)
                        }
                        Some('!') => {
                            self.next_char();
                            Ok(Token::NegativeLookbehind)
                        }
                        Some(c) if c.is_alphabetic() || c == '_' => self.lex_named_group(),
                        _ => Err(Error::parse(
                            self.position,
                            "expected '=', '!', or group name after '(?<'",
                        )),
                    }
                }
                Some('P') => {
                    self.next_char();
                    match self.peek_char() {
                        Some('<') => {
                            self.next_char();
                            self.lex_named_group()
                        }
                        Some('>') => {
                            // Recursive named group: (?P>name)
                            self.next_char();
                            self.lex_recursive_name()
                        }
                        Some('=') => {
                            // Named backreference: (?P=name)
                            self.next_char();
                            self.lex_named_backreference_until_paren()
                        }
                        _ => Err(Error::parse(
                            self.position,
                            "expected '<', '>', or '=' after '(?P'",
                        )),
                    }
                }
                // A run of one or more inline flags, e.g. `(?i)`, `(?es)`,
                // `(?ims)`. Combined flags are parsed together and applied by
                // the parser.
                Some(c) if Self::is_inline_flag_char(c) => self.lex_flag_run(),
                Some('>') => {
                    // Atomic group: (?>...)
                    self.next_char();
                    Ok(Token::AtomicGroup)
                }
                Some('R') => {
                    // Recursive entire pattern: (?R)
                    self.next_char();
                    if self.peek_char() == Some(')') {
                        self.next_char();
                        Ok(Token::RecursivePattern)
                    } else {
                        Err(Error::parse(self.position, "expected ')' after '(?R'"))
                    }
                }
                Some('&') => {
                    // Recursive named group: (?&name)
                    self.next_char();
                    self.lex_recursive_name()
                }
                Some('c') => {
                    // Custom handler: (?call:name)
                    self.next_char();
                    match self.peek_char() {
                        Some('a') => {
                            self.next_char();
                            match self.peek_char() {
                                Some('l') => {
                                    self.next_char();
                                    match self.peek_char() {
                                        Some('l') => {
                                            self.next_char();
                                            // Now expect ':' after 'call'
                                            match self.peek_char() {
                                                Some(':') => {
                                                    self.next_char();
                                                    self.lex_handler_name()
                                                }
                                                _ => Err(Error::parse(
                                                    self.position,
                                                    "expected ':' after '(?call'",
                                                )),
                                            }
                                        }
                                        _ => Err(Error::parse(
                                            self.position,
                                            "expected 'l' after '(?cal'",
                                        )),
                                    }
                                }
                                _ => Err(Error::parse(self.position, "expected 'l' after '(?ca'")),
                            }
                        }
                        _ => Err(Error::parse(self.position, "expected 'a' after '(?c'")),
                    }
                }
                Some(c) if c.is_ascii_digit() => {
                    // Recursive numbered group: (?1), (?2), etc.
                    self.lex_recursive_number()
                }
                _ => Err(Error::parse(
                    self.position,
                    "invalid group syntax after '(?'",
                )),
            }
        } else {
            Ok(Token::OpenParen)
        }
    }

    /// Whether `c` is a recognized inline-flag letter.
    /// `f` (mrab FULLCASE) is accepted but treated as a no-op: full Unicode case
    /// folding isn't implemented, so `(?fi)` degrades to plain `(?i)`.
    fn is_inline_flag_char(c: char) -> bool {
        matches!(
            c,
            'b' | 'e' | 'p' | 'x' | 's' | 'm' | 'U' | 'i' | 'g' | 'u' | 'f' | 'r'
        )
    }

    /// Lex a run of inline flags after `(?`, e.g. `i`, `es`, `ims`, up to `)`.
    /// The leading `(?` has been consumed; the first char is a flag letter.
    fn lex_flag_run(&mut self) -> Result<Token> {
        let mut flags = InlineFlags::default();
        while let Some(c) = self.peek_char() {
            match c {
                'b' => flags.best_match = true,
                'e' => flags.enhance_match = true,
                'p' => flags.posix = true,
                'x' => flags.verbose = true,
                's' => flags.dot_all = true,
                'm' => flags.multi_line = true,
                'U' => flags.ungreedy = true,
                'i' => flags.case_insensitive = true,
                'g' => flags.global = true,
                'u' => flags.unicode = true,
                'r' => flags.reverse = true,
                // FULLCASE: accepted, no-op (see is_inline_flag_char).
                'f' => {}
                ')' => {
                    self.next_char();
                    // Verbose must take effect for the rest of this lex pass.
                    if flags.verbose {
                        self.verbose = true;
                    }
                    return Ok(Token::InlineFlags(flags));
                }
                _ => {
                    return Err(Error::parse(
                        self.position,
                        "unexpected character in inline flags; expected a flag or ')'",
                    ));
                }
            }
            self.next_char();
        }
        Err(Error::parse(
            self.position,
            "unterminated inline flags: expected ')'",
        ))
    }

    /// Lex a named group name.
    fn lex_named_group(&mut self) -> Result<Token> {
        let mut name = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == '>' {
                self.next_char();
                if name.is_empty() {
                    return Err(Error::parse(self.position, "empty group name"));
                }
                return Ok(Token::NamedGroup(name));
            } else if ch.is_alphanumeric() || ch == '_' {
                name.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in group name: '{ch}'"),
                ));
            }
        }

        Err(Error::unclosed("named group", self.position))
    }

    /// Lex a recursive group number: (?1), (?2), etc.
    fn lex_recursive_number(&mut self) -> Result<Token> {
        let mut num = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == ')' {
                self.next_char();
                if num.is_empty() {
                    return Err(Error::parse(self.position, "empty recursive group number"));
                }
                let group_num: usize = num
                    .parse()
                    .map_err(|_| Error::parse(self.position, "invalid recursive group number"))?;
                return Ok(Token::RecursiveGroup(group_num));
            } else if ch.is_ascii_digit() {
                num.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in recursive group: '{ch}'"),
                ));
            }
        }

        Err(Error::unclosed("recursive group", self.position))
    }

    /// Lex a recursive group name: (?&name) or (?P>name)
    fn lex_recursive_name(&mut self) -> Result<Token> {
        let mut name = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == ')' {
                self.next_char();
                if name.is_empty() {
                    return Err(Error::parse(self.position, "empty recursive group name"));
                }
                return Ok(Token::RecursiveNamedGroup(name));
            } else if ch.is_alphanumeric() || ch == '_' {
                name.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in recursive group name: '{ch}'"),
                ));
            }
        }

        Err(Error::unclosed("recursive group", self.position))
    }

    fn lex_handler_name(&mut self) -> Result<Token> {
        let mut name = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == ')' {
                self.next_char();
                if name.is_empty() {
                    return Err(Error::parse(self.position, "empty handler name"));
                }
                return Ok(Token::Handler(name));
            } else if ch.is_alphanumeric() || ch == '_' {
                name.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in handler name: '{ch}'"),
                ));
            }
        }

        Err(Error::unclosed("handler", self.position))
    }

    /// Lex an escape sequence.
    fn lex_escape(&mut self) -> Result<Token> {
        let Some(ch) = self.next_char() else {
            return Err(Error::parse(self.position, "unexpected end after '\\'"));
        };

        match ch {
            // Named classes
            'd' => Ok(Token::NamedClass(NamedClassToken::Digit)),
            'D' => Ok(Token::NamedClass(NamedClassToken::NotDigit)),
            'w' => Ok(Token::NamedClass(NamedClassToken::Word)),
            'W' => Ok(Token::NamedClass(NamedClassToken::NotWord)),
            's' => Ok(Token::NamedClass(NamedClassToken::Whitespace)),
            'S' => Ok(Token::NamedClass(NamedClassToken::NotWhitespace)),
            'b' => Ok(Token::NamedClass(NamedClassToken::WordBoundary)),
            'B' => Ok(Token::NamedClass(NamedClassToken::NotWordBoundary)),

            // Named list \L<name>
            'L' => self.lex_named_list(),

            // Common escapes
            'n' => Ok(Token::Escaped('\n')),
            'r' => Ok(Token::Escaped('\r')),
            't' => Ok(Token::Escaped('\t')),
            'f' => Ok(Token::Escaped('\x0C')),
            'v' => Ok(Token::Escaped('\x0B')),
            '0' => Ok(Token::Escaped('\0')),

            // Backreference (1-9)
            '1'..='9' => {
                let mut num = ch.to_digit(10).unwrap() as usize;
                // Check for multi-digit backreference
                while let Some(next_ch) = self.peek_char() {
                    if let Some(digit) = next_ch.to_digit(10) {
                        num = num * 10 + digit as usize;
                        self.next_char();
                    } else {
                        break;
                    }
                }
                Ok(Token::Backreference(num))
            }

            // Hex escape \xHH
            'x' => self.lex_hex_escape(),

            // Unicode escape \u{HHHH} or \uHHHH
            'u' => self.lex_unicode_escape(),

            // Escaped metacharacters and literals
            '\\' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '.' | '*' | '+' | '?'
            | '~' | '-' | '/' => Ok(Token::Escaped(ch)),

            // \K - reset match start
            'K' => Ok(Token::ResetMatchStart),

            // Named backreference \k<name> or \k{name}
            'k' => self.lex_named_backreference(),

            _ => Err(Error::invalid_escape(ch, self.position - 1)),
        }
    }

    /// Lex a hex escape \xHH.
    fn lex_hex_escape(&mut self) -> Result<Token> {
        let mut hex = String::new();

        for _ in 0..2 {
            match self.next_char() {
                Some(ch) if ch.is_ascii_hexdigit() => hex.push(ch),
                Some(ch) => {
                    return Err(Error::parse(
                        self.position,
                        format!("invalid hex digit: '{ch}'"),
                    ));
                }
                None => return Err(Error::parse(self.position, "incomplete hex escape")),
            }
        }

        let code = u8::from_str_radix(&hex, 16).unwrap();
        Ok(Token::Escaped(code as char))
    }

    /// Lex a unicode escape \u{HHHH} or \uHHHH.
    fn lex_unicode_escape(&mut self) -> Result<Token> {
        let braced = self.peek_char() == Some('{');
        if braced {
            self.next_char();
        }

        let mut hex = String::new();
        let max_digits = if braced { 6 } else { 4 };

        for i in 0..max_digits {
            match self.peek_char() {
                Some('}') if braced => {
                    self.next_char();
                    break;
                }
                Some(ch) if ch.is_ascii_hexdigit() => {
                    hex.push(ch);
                    self.next_char();
                }
                Some(_) if !braced && i >= 4 => break,
                Some(ch) => {
                    return Err(Error::parse(
                        self.position,
                        format!("invalid unicode digit: '{ch}'"),
                    ));
                }
                None => return Err(Error::parse(self.position, "incomplete unicode escape")),
            }
        }

        if braced && self.peek_char() != Some('}') && hex.len() < max_digits {
            return Err(Error::unclosed("unicode escape", self.position));
        }

        let code = u32::from_str_radix(&hex, 16)
            .map_err(|_| Error::parse(self.position, format!("invalid unicode value: {hex}")))?;

        char::from_u32(code)
            .ok_or_else(|| {
                Error::parse(
                    self.position,
                    format!("invalid unicode code point: U+{code:04X}"),
                )
            })
            .map(Token::Escaped)
    }

    /// Lex a named list reference \L<name>.
    /// Lex the name of a `(?P=name)` backreference, up to the closing `)`.
    /// The leading `(?P=` has already been consumed.
    fn lex_named_backreference_until_paren(&mut self) -> Result<Token> {
        let mut name = String::new();
        while let Some(ch) = self.peek_char() {
            if ch == ')' {
                self.next_char(); // consume ')'
                if name.is_empty() {
                    return Err(Error::parse(self.position, "empty backreference name"));
                }
                return Ok(Token::NamedBackreference(name));
            }
            if ch.is_alphanumeric() || ch == '_' {
                name.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in backreference name: '{ch}'"),
                ));
            }
        }
        Err(Error::unclosed("named backreference", self.position))
    }

    /// Lex a named backreference: `\k<name>` or `\k{name}`.
    fn lex_named_backreference(&mut self) -> Result<Token> {
        let close = match self.peek_char() {
            Some('<') => '>',
            Some('{') => '}',
            _ => {
                return Err(Error::parse(
                    self.position,
                    "expected '<' or '{' after '\\k'",
                ));
            }
        };
        self.next_char(); // consume the opening delimiter

        let mut name = String::new();
        while let Some(ch) = self.peek_char() {
            if ch == close {
                self.next_char(); // consume the closing delimiter
                if name.is_empty() {
                    return Err(Error::parse(self.position, "empty backreference name"));
                }
                return Ok(Token::NamedBackreference(name));
            }
            if ch.is_alphanumeric() || ch == '_' {
                name.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in backreference name: '{ch}'"),
                ));
            }
        }

        Err(Error::unclosed("named backreference", self.position))
    }

    fn lex_named_list(&mut self) -> Result<Token> {
        // Expect < after \L
        let Some(ch) = self.peek_char() else {
            return Err(Error::parse(self.position, "unexpected end after '\\L'"));
        };

        if ch != '<' {
            return Err(Error::parse(self.position, "expected '<' after '\\L'"));
        }
        self.next_char(); // consume '<'

        // Read the name until we find '>'
        let mut name = String::new();
        while let Some(ch) = self.peek_char() {
            if ch == '>' {
                self.next_char(); // consume '>'
                return Ok(Token::NamedList(name));
            }
            if ch.is_alphanumeric() || ch == '_' {
                name.push(ch);
                self.next_char();
            } else {
                return Err(Error::parse(
                    self.position,
                    format!("invalid character in named list: '{ch}'"),
                ));
            }
        }

        Err(Error::unclosed("named list", self.position))
    }

    /// Peek at the next token without consuming it.
    ///
    /// # Errors
    /// Returns an error if an invalid escape sequence is encountered.
    pub fn peek_token(&mut self) -> Result<Token> {
        let saved_position = self.position;
        let saved_chars = self.chars.clone();
        let token = self.next_token()?;
        self.position = saved_position;
        self.chars = saved_chars;
        Ok(token)
    }

    /// Check if we've reached the end of input.
    pub fn is_eof(&mut self) -> bool {
        self.peek_char().is_none()
    }

    /// Get remaining input from current position.
    #[must_use]
    pub fn remaining(&self) -> &'a str {
        &self.input[self.position..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_chars() {
        let mut lexer = Lexer::new("abc");
        assert_eq!(lexer.next_token().unwrap(), Token::Char('a'));
        assert_eq!(lexer.next_token().unwrap(), Token::Char('b'));
        assert_eq!(lexer.next_token().unwrap(), Token::Char('c'));
        assert_eq!(lexer.next_token().unwrap(), Token::Eof);
    }

    #[test]
    fn test_metacharacters() {
        let mut lexer = Lexer::new(".*+?");
        assert_eq!(lexer.next_token().unwrap(), Token::Dot);
        assert_eq!(lexer.next_token().unwrap(), Token::Star);
        assert_eq!(lexer.next_token().unwrap(), Token::Plus);
        assert_eq!(lexer.next_token().unwrap(), Token::Question);
    }

    #[test]
    fn test_escapes() {
        let mut lexer = Lexer::new(r"\d\w\s\n\t");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::NamedClass(NamedClassToken::Digit)
        );
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::NamedClass(NamedClassToken::Word)
        );
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::NamedClass(NamedClassToken::Whitespace)
        );
        assert_eq!(lexer.next_token().unwrap(), Token::Escaped('\n'));
        assert_eq!(lexer.next_token().unwrap(), Token::Escaped('\t'));
    }

    #[test]
    fn test_backreference() {
        let mut lexer = Lexer::new(r"\1\12");
        assert_eq!(lexer.next_token().unwrap(), Token::Backreference(1));
        assert_eq!(lexer.next_token().unwrap(), Token::Backreference(12));
    }

    #[test]
    fn test_groups() {
        let mut lexer = Lexer::new("(a)(?:b)(?=c)(?!d)");
        assert_eq!(lexer.next_token().unwrap(), Token::OpenParen);
        assert_eq!(lexer.next_token().unwrap(), Token::Char('a'));
        assert_eq!(lexer.next_token().unwrap(), Token::CloseParen);
        assert_eq!(lexer.next_token().unwrap(), Token::NonCapturing);
        assert_eq!(lexer.next_token().unwrap(), Token::Char('b'));
        assert_eq!(lexer.next_token().unwrap(), Token::CloseParen);
        assert_eq!(lexer.next_token().unwrap(), Token::PositiveLookahead);
        assert_eq!(lexer.next_token().unwrap(), Token::Char('c'));
        assert_eq!(lexer.next_token().unwrap(), Token::CloseParen);
        assert_eq!(lexer.next_token().unwrap(), Token::NegativeLookahead);
        assert_eq!(lexer.next_token().unwrap(), Token::Char('d'));
        assert_eq!(lexer.next_token().unwrap(), Token::CloseParen);
    }

    #[test]
    fn test_named_group() {
        let mut lexer = Lexer::new("(?<name>a)(?P<other>b)");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::NamedGroup("name".into())
        );
        assert_eq!(lexer.next_token().unwrap(), Token::Char('a'));
        assert_eq!(lexer.next_token().unwrap(), Token::CloseParen);
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::NamedGroup("other".into())
        );
    }

    #[test]
    fn test_fuzziness_marker() {
        let mut lexer = Lexer::new("hello~2");
        for ch in "hello".chars() {
            assert_eq!(lexer.next_token().unwrap(), Token::Char(ch));
        }
        assert_eq!(lexer.next_token().unwrap(), Token::Tilde);
        assert_eq!(lexer.next_token().unwrap(), Token::Char('2'));
    }
}
