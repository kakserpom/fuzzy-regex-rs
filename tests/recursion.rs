//! Tests for recursive patterns: `(?R)`, `(?0)` (whole pattern), `(?1)`
//! (numbered group), and `(?&name)` / `(?P>name)` (named group).
//!
//! Recursion is executed by the backtracking engine as a subroutine call stack
//! integrated into the single backtracking search, so choices made inside a
//! recursive call are revisited when a later part of the match fails.

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn balanced_parentheses_via_capital_r() {
    let re = FuzzyRegex::new(r"\((?:[^()]|(?R))*\)").unwrap();
    assert_eq!(span(&re, "(a(b)c)"), Some((0, 7)));
    assert_eq!(span(&re, "((()))"), Some((0, 6)));
    assert_eq!(span(&re, "()"), Some((0, 2)));
    // Deep nesting does not overflow.
    assert_eq!(span(&re, "(((((((((())))))))))"), Some((0, 20)));
}

#[test]
fn balanced_parentheses_unanchored() {
    let re = FuzzyRegex::new(r"\((?:[^()]|(?R))*\)").unwrap();
    // Leftmost balanced group within surrounding text.
    assert_eq!(span(&re, "x(a(b)c)y"), Some((1, 8)));
}

#[test]
fn whole_pattern_recursion_zero() {
    // (?0) is the whole pattern, same as (?R).
    let re = FuzzyRegex::new(r"a(?0)?b").unwrap();
    assert_eq!(span(&re, "ab"), Some((0, 2)));
    assert_eq!(span(&re, "aabb"), Some((0, 4)));
    assert_eq!(span(&re, "aaabbb"), Some((0, 6)));
}

#[test]
fn numbered_group_recursion() {
    let re = FuzzyRegex::new(r"(a(?1)?b)").unwrap();
    assert_eq!(span(&re, "aabb"), Some((0, 4)));
    assert_eq!(span(&re, "ab"), Some((0, 2)));
    // (The exact captured value of a group under recursion is left unspecified —
    // engines disagree on which recursion level "wins" — so only the overall
    // span is asserted here.)
}

#[test]
fn named_group_recursion() {
    let re = FuzzyRegex::new(r"(?<p>a(?&p)?b)").unwrap();
    assert_eq!(span(&re, "aaabbb"), Some((0, 6)));
    assert_eq!(span(&re, "ab"), Some((0, 2)));
    // (?P>name) is the same call, Python style.
    let re2 = FuzzyRegex::new(r"(?P<p>a(?P>p)?b)").unwrap();
    assert_eq!(span(&re2, "aabb"), Some((0, 4)));
}

#[test]
fn unbalanced_input_matches_the_leftmost_balanced_part() {
    let re = FuzzyRegex::new(r"a(?0)?b").unwrap();
    // "aab": the balanced sub-match is "ab" at position 1.
    assert_eq!(span(&re, "aab"), Some((1, 3)));
    // "abb": "ab" at position 0.
    assert_eq!(span(&re, "abb"), Some((0, 2)));
}

#[test]
fn recursion_with_fuzziness() {
    // Recursion inside a fuzzy group still matches the exact case.
    let re = FuzzyRegex::new(r"(?:a(?0)?b){e<=1}").unwrap();
    assert_eq!(span(&re, "aabb"), Some((0, 4)));
}

#[test]
fn left_recursion_does_not_hang() {
    // A mandatory self-call at the same position (no progress) must terminate.
    let re = FuzzyRegex::new(r"a(?0)b").unwrap();
    assert!(re.find("ab").is_none());
    let r2 = FuzzyRegex::new(r"(?R)").unwrap();
    assert!(r2.find("x").is_none());
}
