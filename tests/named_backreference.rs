//! Tests for named backreferences: `\k<name>`, `\k{name}`, and `(?P=name)`.
//!
//! Numeric backreferences (`\1`) were already supported via the backtracking
//! engine; these tests cover the named forms, which resolve the name to the
//! group's index at parse time and then reuse the same matching machinery.

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn k_angle_named_backreference() {
    let re = FuzzyRegex::new(r"(?P<x>\w+) \k<x>").unwrap();
    assert_eq!(span(&re, "hi hi"), Some((0, 5)));
    assert!(re.find("hi ho").is_none());
}

#[test]
fn k_brace_named_backreference() {
    let re = FuzzyRegex::new(r"(?P<x>\w+) \k{x}").unwrap();
    assert_eq!(span(&re, "yo yo"), Some((0, 5)));
    assert!(re.find("yo no").is_none());
}

#[test]
fn python_style_named_backreference() {
    let re = FuzzyRegex::new(r"(?P<w>\w+) (?P=w)").unwrap();
    assert_eq!(span(&re, "hi hi"), Some((0, 5)));
    assert!(re.find("hi no").is_none());
}

#[test]
fn angle_group_with_named_backreference() {
    // `(?<name>...)` group form (no P) with `\k<name>`.
    let re = FuzzyRegex::new(r"(?<x>\w+) \k<x>").unwrap();
    assert_eq!(span(&re, "ab ab"), Some((0, 5)));
}

#[test]
fn named_backreference_matches_numeric_equivalent() {
    let named = FuzzyRegex::new(r"(?P<x>\w+) \k<x>").unwrap();
    let numeric = FuzzyRegex::new(r"(\w+) \1").unwrap();
    for t in ["hi hi", "hi ho", "aaa aaa", "x y"] {
        assert_eq!(span(&named, t), span(&numeric, t), "on {t:?}");
    }
}

#[test]
fn named_backreference_with_fuzziness() {
    // A fuzzy named backreference allows the repeated text to differ by 1 edit.
    let re = FuzzyRegex::new(r"(?P<x>abc) \k<x>{e<=1}").unwrap();
    assert_eq!(span(&re, "abc abc"), Some((0, 7))); // exact repeat
    assert_eq!(span(&re, "abc abx"), Some((0, 7))); // 1 substitution
    assert!(re.find("abc xyz").is_none()); // too different
}

#[test]
fn unknown_name_is_an_error() {
    assert!(FuzzyRegex::new(r"(?P<x>a)\k<y>").is_err());
    assert!(FuzzyRegex::new(r"(?P<x>a)(?P=y)").is_err());
    assert!(FuzzyRegex::new(r"\k<none>").is_err());
}

#[test]
fn malformed_named_backreference_is_an_error() {
    assert!(FuzzyRegex::new(r"(?P<x>a)\k").is_err()); // no delimiter
    assert!(FuzzyRegex::new(r"(?P<x>a)\k<>").is_err()); // empty name
    assert!(FuzzyRegex::new(r"(?P<x>a)\k<x").is_err()); // unclosed
}

#[test]
fn multiple_named_groups_and_backreferences() {
    let re = FuzzyRegex::new(r"(?P<a>\w)(?P<b>\w)\k<b>\k<a>").unwrap();
    // a=x, b=y, then y, then x  ->  "xyyx"
    assert_eq!(span(&re, "xyyx"), Some((0, 4)));
    assert!(re.find("xyxy").is_none());
}
