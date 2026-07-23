//! Tests for forward references: a backreference to a capture group defined
//! *later* in the pattern (`\1(a)`, `\k<x>(?P<x>a)`, `(?r)\1(a)`).
//!
//! A pre-scan counts groups/names before parsing, so the reference resolves;
//! when the referenced group has not captured yet, the reference matches the
//! empty string (matching Python's `regex` module for unset backreferences).

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn numeric_forward_reference_compiles_and_matches() {
    // `\1` is unset when reached, so it matches empty; then `(a)` matches "a".
    let re = FuzzyRegex::new(r"\1(a)").unwrap();
    assert_eq!(span(&re, "a"), Some((0, 1)));
    let caps = re.captures("a").unwrap();
    assert_eq!(caps.get(1).unwrap().as_str(), "a");
}

#[test]
fn named_forward_reference() {
    let re = FuzzyRegex::new(r"\k<x>(?P<x>a)").unwrap();
    assert_eq!(span(&re, "a"), Some((0, 1)));
}

#[test]
fn reverse_mode_forward_reference_compiles() {
    // Corpus L4441 / L2762 shapes: forward reference under `(?r)`.
    for pat in [r"(?r)\1{e<=1:[a-z]}(a)", r"(?r)(\2{e<=1}) (\w+)"] {
        let re = FuzzyRegex::new(pat).expect("should compile");
        // Should not panic on a search.
        let _ = re.find("aa bb");
    }
}

#[test]
fn forward_reference_to_nonexistent_group_still_errors() {
    // A group index / name that never appears anywhere is a compile error.
    assert!(FuzzyRegex::new(r"(a)\2").is_err()); // only 1 group total
    assert!(FuzzyRegex::new(r"\1").is_err()); // no groups at all
    assert!(FuzzyRegex::new(r"\3(a)(b)").is_err()); // only 2 groups total
    assert!(FuzzyRegex::new(r"\k<z>(a)").is_err()); // name never defined
}

#[test]
fn backward_reference_unaffected() {
    let re = FuzzyRegex::new(r"(a)\1").unwrap();
    assert_eq!(span(&re, "aa"), Some((0, 2)));
    assert!(re.find("ab").is_none());

    let word = FuzzyRegex::new(r"(\w+) \1").unwrap();
    assert_eq!(span(&word, "hi hi"), Some((0, 5)));
    assert!(word.find("hi ho").is_none());
}

#[test]
fn reference_to_group_on_untaken_branch_matches_empty() {
    // When matching the "b" branch, group 1 never captures, so `\1` (in the
    // sequence) matches empty rather than killing the thread.
    let re = FuzzyRegex::new(r"(?:(a)|b)\1c").unwrap();
    // "bc": takes the `b` branch, group 1 unset, `\1` empty, then "c".
    assert_eq!(span(&re, "bc"), Some((0, 2)));
    // "aac": takes the `a` branch (group1="a"), `\1` matches "a", then "c".
    assert_eq!(span(&re, "aac"), Some((0, 3)));
}
