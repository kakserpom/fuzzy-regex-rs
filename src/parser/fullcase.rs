//! Full case-folding (`(?f)`) pattern rewrite.
//!
//! Unicode *full* case folding is 1↔N: `ß` folds to `"ss"`, the ligature `ﬀ`
//! to `"ff"`, etc. Rather than teach every matching engine to consume a
//! variable number of characters, `(?f)` (only meaningful together with `(?i)`)
//! is implemented as an AST rewrite that makes both directions matchable through
//! the ordinary engines:
//!
//! * **Forward** — a character whose fold expands (e.g. `ß`) is rewritten to an
//!   alternation of itself and its fold sequence: `ß` → `(?:ß|ss)`. Combined
//!   with case-insensitive matching this matches `ß`, `ẞ`, `ss`, `SS`, ….
//! * **Reverse** — a run of literal characters equal to some character's fold
//!   (e.g. `"ss"`) gains that character as an alternative: `ss` → `(?:ss|ß)`.
//!
//! The rewrite runs only when `(?f)` and `(?i)` are both set, so it never
//! affects patterns that do not opt in.

use super::ast::{Ast, Fuzziness};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Full case fold of a single character, as a string (possibly multi-char).
fn fold_char(c: char) -> String {
    caseless::default_case_fold_str(c.encode_utf8(&mut [0u8; 4]))
}

/// The fold of `c` if it expands to more than one character, else `None`.
fn multichar_fold(c: char) -> Option<String> {
    let f = fold_char(c);
    (f.chars().nth(1).is_some()).then_some(f)
}

/// `fold string -> characters that fold to it` (e.g. `"ss" -> ['ß', 'ẞ']`),
/// with the longest key length in characters. Built once by scanning the
/// Unicode blocks that contain multi-character case foldings (Latin, Greek,
/// Armenian, and the ligature block).
fn reverse_fold_map() -> &'static (HashMap<String, Vec<char>>, usize) {
    static MAP: OnceLock<(HashMap<String, Vec<char>>, usize)> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m: HashMap<String, Vec<char>> = HashMap::new();
        let mut max_len = 0usize;
        for cp in (0x00u32..0x2100).chain(0xFB00..0xFB50) {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            if let Some(f) = multichar_fold(c) {
                max_len = max_len.max(f.chars().count());
                m.entry(f).or_default().push(c);
            }
        }
        (m, max_len)
    })
}

/// Build an AST that matches the literal string `s` exactly (fuzziness inherited).
fn literal(s: &str) -> Ast {
    Ast::Literal {
        text: s.to_string(),
        fuzziness: Fuzziness::Inherited,
    }
}

/// Forward expansion for a single character.
fn expand_char(c: char) -> Ast {
    match multichar_fold(c) {
        Some(fold) => Ast::Alternation(vec![Ast::Char(c), literal(&fold)]),
        None => Ast::Char(c),
    }
}

/// Rewrite a literal string: expand fold-expanding characters (forward) and
/// offer the collapsed character for runs equal to a fold (reverse). Wraps the
/// result in the literal's original fuzziness so `{e<=1}` still applies to the
/// whole piece.
fn expand_literal(text: &str, fuzziness: Fuzziness) -> Ast {
    let (rev, max_key_chars) = reverse_fold_map();
    let chars: Vec<char> = text.chars().collect();
    let mut units: Vec<Ast> = Vec::new();
    // Consecutive non-folding characters are kept together as one literal run so
    // the piece's structure (and its fuzzy behaviour) is preserved.
    let mut plain = String::new();
    let mut changed = false;
    let flush_plain = |plain: &mut String, units: &mut Vec<Ast>| {
        if !plain.is_empty() {
            units.push(literal(plain));
            plain.clear();
        }
    };

    let mut i = 0;
    while i < chars.len() {
        // Reverse: try the longest run [i..j] (>=2 chars) that equals a fold.
        let max_j = (i + max_key_chars).min(chars.len());
        let mut matched = false;
        for j in (i + 2..=max_j).rev() {
            let run: String = chars[i..j].iter().collect();
            if let Some(collapsed) = rev.get(&run) {
                flush_plain(&mut plain, &mut units);
                let mut alts = vec![literal(&run)];
                alts.extend(collapsed.iter().map(|&c| Ast::Char(c)));
                units.push(Ast::Alternation(alts));
                i = j;
                matched = true;
                changed = true;
                break;
            }
        }
        if matched {
            continue;
        }
        // Forward: a character that expands under folding.
        if let Some(fold) = multichar_fold(chars[i]) {
            flush_plain(&mut plain, &mut units);
            units.push(Ast::Alternation(vec![Ast::Char(chars[i]), literal(&fold)]));
            changed = true;
        } else {
            plain.push(chars[i]);
        }
        i += 1;
    }

    // No folding applied: leave the literal exactly as it was.
    if !changed {
        return Ast::Literal {
            text: text.to_string(),
            fuzziness,
        };
    }
    flush_plain(&mut plain, &mut units);

    let body = if units.len() == 1 {
        units.pop().unwrap()
    } else {
        Ast::Concat(units)
    };

    if matches!(fuzziness, Fuzziness::Inherited) {
        body
    } else {
        Ast::NonCapturingGroup {
            expr: Box::new(body),
            fuzziness,
        }
    }
}

/// Rewrite `ast` for full case folding. Only call this when both `(?f)` and
/// `(?i)` are active.
pub fn apply(ast: Ast) -> Ast {
    match ast {
        Ast::Char(c) => expand_char(c),
        Ast::Literal { text, fuzziness } => expand_literal(&text, fuzziness),
        Ast::Concat(v) => Ast::Concat(v.into_iter().map(apply).collect()),
        Ast::Alternation(v) => Ast::Alternation(v.into_iter().map(apply).collect()),
        Ast::Quantified {
            expr,
            quantifier,
            greedy,
        } => Ast::Quantified {
            expr: Box::new(apply(*expr)),
            quantifier,
            greedy,
        },
        Ast::Group { index, name, expr } => Ast::Group {
            index,
            name,
            expr: Box::new(apply(*expr)),
        },
        Ast::NonCapturingGroup { expr, fuzziness } => Ast::NonCapturingGroup {
            expr: Box::new(apply(*expr)),
            fuzziness,
        },
        Ast::Lookahead { positive, expr } => Ast::Lookahead {
            positive,
            expr: Box::new(apply(*expr)),
        },
        Ast::Lookbehind { positive, expr } => Ast::Lookbehind {
            positive,
            expr: Box::new(apply(*expr)),
        },
        Ast::AtomicGroup { expr } => Ast::AtomicGroup {
            expr: Box::new(apply(*expr)),
        },
        // Char classes, anchors, backreferences, named lists, recursion, and
        // handlers are left unchanged: full folding applies to literal text, and
        // a character class matches a single code point (mrab does not expand
        // class members).
        other => other,
    }
}
