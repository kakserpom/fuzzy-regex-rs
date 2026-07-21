//! Regression tests for `find()` fast-path hijacking.
//!
//! The specialized linear-scan "shape" fast paths in `find()` (currency,
//! class-plus-with-literal, digit-sequence-with-separator) used to grab complex
//! anchored / group-repeating patterns they cannot handle, returning truncated
//! or missing matches that disagreed with `find_iter()`. These assert that
//! `find()` now agrees with `find_iter().next()` and yields the correct span.

use fuzzy_regex::FuzzyRegex;

fn find_span(pat: &str, input: &str) -> Option<(usize, usize)> {
    FuzzyRegex::new(pat)
        .unwrap()
        .find(input)
        .map(|m| (m.start(), m.end()))
}

fn iter_first(pat: &str, input: &str) -> Option<(usize, usize)> {
    FuzzyRegex::new(pat)
        .unwrap()
        .find_iter(input)
        .next()
        .map(|m| (m.start(), m.end()))
}

/// `find()` must agree with the leftmost match reported by `find_iter()`.
fn assert_consistent(pat: &str, input: &str, expected: Option<(usize, usize)>) {
    let f = find_span(pat, input);
    let it = iter_first(pat, input);
    assert_eq!(f, it, "find != find_iter.next for [{pat}] on {input:?}");
    assert_eq!(f, expected, "wrong span for [{pat}] on {input:?}");
}

#[test]
fn anchored_class_with_repeated_group() {
    // class then a repeated multi-atom group, end-anchored
    assert_consistent(r"^\d(?:,\d)*$", "1,2,3", Some((0, 5)));
    assert_consistent(r"^\d(?:,\d)*$", "1", Some((0, 1)));
    assert_consistent(r"^\d{1,3}(?:,\d{3})*$", "10", Some((0, 2)));
    assert_consistent(r"^\d{1,3}(?:,\d{3})*$", "10,112,111", Some((0, 10)));
    assert_consistent(r"^\d{1,3}(?:,\d{3})*$", "1,234,567", Some((0, 9)));
    assert_consistent(r"^\d+(?:,\d+)*$", "1,22,333", Some((0, 8)));
    assert_consistent(r"^X(?:,\d{3})*$", "X,123,456", Some((0, 9)));
    assert_consistent(r"^X(?:,\d{3})*$", "X", Some((0, 1)));
    assert_consistent(r"^(?:,\d{3})*$", ",123,456", Some((0, 8)));
}

#[test]
fn unanchored_class_with_repeated_group() {
    // greedy: match all of "1,2,3", not just "1,2"
    assert_consistent(r"\d(?:,\d)*", "1,2,3", Some((0, 5)));
}

#[test]
fn money_pattern_with_grouped_thousands() {
    let pat = r"^(?:\$ )?\d{1,3}(?:,\d{3})*(?:\.\d{2})$";
    assert_consistent(pat, "$ 10,112.11", Some((0, 11)));
    assert_consistent(pat, "$ 10,112,111.12", Some((0, 15)));
    assert_consistent(pat, "10,112.11", Some((0, 9)));
}

#[test]
fn anchored_currency_no_longer_hijacked() {
    // Leading `$` used to trigger the currency fast path and return None.
    assert_consistent(r"^\$\d+$", "$100", Some((0, 4)));
    assert_consistent(r"^\$\d+$", "$1", Some((0, 2)));
    assert_consistent(r"^\$\d+\.\d{2}$", "$100.50", Some((0, 7)));
    assert_consistent(r"^\$\d+\.\d{2}$", "$1.00", Some((0, 5)));
}

#[test]
fn leading_literal_not_treated_as_email() {
    // `\$\d+` leads with a literal; the class-plus-with-literal (email) fast
    // path used to claim it and return None. Currency handles the unanchored
    // form; both must agree with find_iter.
    assert_consistent(r"\$\d+", "cost $50", Some((5, 8)));
    assert_consistent(r"\$[\d,]+\.\d{2}", "buy $1,234.50 now", Some((4, 13)));
}

#[test]
fn intended_fast_paths_still_work() {
    // The fixes must not break the patterns these fast paths target.
    assert_consistent(r"\w+@\w+", "hi user@host x", Some((3, 12)));
    assert_consistent(r"\d{4}-\d{2}-\d{2}", "on 2020-01-02!", Some((3, 13)));
    assert_consistent(
        r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
        "ip 192.168.0.1 end",
        Some((3, 14)),
    );
    assert_consistent(r"\$\d+", "cost $50 total", Some((5, 8)));
}
