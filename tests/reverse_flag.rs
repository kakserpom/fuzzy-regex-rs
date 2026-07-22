//! Tests for the `(?r)` reverse-matching inline flag.
//!
//! `(?r)` makes the engine search from the end of the text toward the start:
//! `find` returns the rightmost match, `find_iter` yields matches right-to-left,
//! and `captures` returns the rightmost match's groups. Match *existence*
//! (`is_match`) is direction-independent.

use fuzzy_regex::FuzzyRegex;

fn span(m: Option<fuzzy_regex::Match>) -> Option<(usize, usize)> {
    m.map(|m| (m.start(), m.end()))
}

fn spans(re: &FuzzyRegex, text: &str) -> Vec<(usize, usize)> {
    re.find_iter(text).map(|m| (m.start(), m.end())).collect()
}

#[test]
fn reverse_find_returns_rightmost() {
    let fwd = FuzzyRegex::new("ab").unwrap();
    let rev = FuzzyRegex::new("(?r)ab").unwrap();
    let text = "ab_ab_ab";
    assert_eq!(span(fwd.find(text)), Some((0, 2)));
    assert_eq!(span(rev.find(text)), Some((6, 8)));
}

#[test]
fn reverse_find_iter_is_right_to_left() {
    let fwd = FuzzyRegex::new("ab").unwrap();
    let rev = FuzzyRegex::new("(?r)ab").unwrap();
    let text = "ab_ab_ab";
    assert_eq!(spans(&fwd, text), vec![(0, 2), (3, 5), (6, 8)]);
    assert_eq!(spans(&rev, text), vec![(6, 8), (3, 5), (0, 2)]);
    // Same set of matches, just reversed order.
    let mut r = spans(&rev, text);
    r.reverse();
    assert_eq!(r, spans(&fwd, text));
}

#[test]
fn reverse_greedy_quantifier_matches_rightmost_run() {
    let rev = FuzzyRegex::new("(?r)x{6}").unwrap();
    // Forward would match [0,6]; reverse takes the rightmost six x's.
    assert_eq!(span(rev.find("xxxxxxxxx")), Some((3, 9)));
}

#[test]
fn reverse_is_match_is_direction_independent() {
    for pat in ["ab", "(?r)ab", "x{6}", "(?r)x{6}"] {
        let re = FuzzyRegex::new(pat).unwrap();
        assert!(re.is_match("zz_ab_x xxxxxx"));
        assert!(!FuzzyRegex::new(pat).unwrap().is_match("nothing here"));
    }
}

#[test]
fn reverse_captures_returns_rightmost() {
    let rev = FuzzyRegex::new(r"(?r)(\d)").unwrap();
    let caps = rev.captures("1_2_3").expect("a digit matches");
    let g0 = caps.get(0).unwrap();
    assert_eq!((g0.start(), g0.end()), (4, 5));
    assert_eq!(caps.get(1).unwrap().as_str(), "3");
}

#[test]
fn reverse_with_fuzzy_non_capturing_group_compiles_and_matches() {
    // Reverse + fuzzy over a non-capturing group (corpus L3944 family shape).
    let re = FuzzyRegex::new(r"(?r)(?:x{6}){e<=1}").unwrap();
    // Exact six-x run.
    assert_eq!(span(re.find("xxxxxx")), Some((0, 6)));
    // Five x's: one deletion is within budget.
    assert!(re.is_match("xxxxx"));
}

#[test]
fn reverse_combines_with_other_flags() {
    // (?er): ENHANCEMATCH + reverse.
    let er = FuzzyRegex::new(r"(?er)(?:ab){e<=1}").unwrap();
    assert!(er.is_match("xabx"));
    // (?ri): reverse + case-insensitive.
    let ri = FuzzyRegex::new(r"(?ri)ab").unwrap();
    assert_eq!(span(ri.find("AB_ab_Ab")), Some((6, 8)));
}

#[test]
fn reverse_lookaround_compiles() {
    // Corpus L3944-L3951: reverse + lookaround + fuzzy over a non-capturing group.
    for pat in [
        r"(?r)(?:ESTONIA(?!\w)){e<=1}",
        r"(?r)(?:ESTONIA(?=\W)){e<=1}",
        r"(?r)(?:(?<!\w)ESTONIA){e<=1}",
        r"(?r)(?:(?<=\W)ESTONIA){e<=1}",
    ] {
        assert!(FuzzyRegex::new(pat).is_ok(), "should compile: {pat}");
    }
}

#[test]
fn reverse_via_builder() {
    use fuzzy_regex::FuzzyRegexBuilder;
    let rev = FuzzyRegexBuilder::new("ab").reverse(true).build().unwrap();
    assert_eq!(span(rev.find("ab_ab_ab")), Some((6, 8)));
    assert_eq!(spans(&rev, "ab_ab_ab"), vec![(6, 8), (3, 5), (0, 2)]);
}

#[test]
fn reverse_no_match_returns_none() {
    let rev = FuzzyRegex::new("(?r)zzz").unwrap();
    assert!(rev.find("abcdef").is_none());
    assert!(rev.find_iter("abcdef").next().is_none());
}
