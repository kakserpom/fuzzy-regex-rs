//! Tests for mrab-style fuzzy quantifiers on (unnamed) capturing groups,
//! e.g. `(abc){e<=1}`. Previously only non-capturing `(?:abc){e<=1}` and named
//! `(?P<n>abc){e<=1}` groups accepted the `{e}`/`{i<=1}` fuzziness syntax; the
//! unnamed capturing path only handled the `~N` form.

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

fn group1<'t>(re: &FuzzyRegex, text: &'t str) -> Option<String> {
    re.captures(text)
        .and_then(|c| c.get(1).map(|g| g.as_str().to_string()))
}

#[test]
fn capturing_group_fuzzy_compiles_and_matches() {
    let re = FuzzyRegex::new(r"(abc){e<=1}").unwrap();
    assert_eq!(span(&re, "abc"), Some((0, 3))); // exact
    assert_eq!(span(&re, "abx"), Some((0, 3))); // 1 substitution
    assert_eq!(span(&re, "ab"), Some((0, 2))); // 1 deletion
    assert_eq!(span(&re, "abcd"), Some((0, 3))); // trailing char not consumed
}

#[test]
fn capturing_group_fuzzy_populates_the_group() {
    let re = FuzzyRegex::new(r"(abc){e<=1}").unwrap();
    assert_eq!(group1(&re, "abc").as_deref(), Some("abc"));
    assert_eq!(group1(&re, "abx").as_deref(), Some("abx"));
    assert_eq!(group1(&re, "ab").as_deref(), Some("ab"));
}

#[test]
fn capturing_matches_named_span() {
    // An unnamed capturing group must match exactly like the named form: both
    // are captures compiled via a shared fuzzy non-capturing group (single
    // `fuzzy_group_id`, group-level error accounting). (The default engine has
    // a pre-existing leading-insertion quirk — on "xabc" the capture forms
    // return the leftmost (0,4) with a leading insertion while `(?:abc){e<=1}`
    // returns the exact (1,4) — so the non-capturing form is not asserted
    // equal here; in mrab-compat mode all three agree with mrab's (1,4).)
    let cap = FuzzyRegex::new(r"(abc){e<=1}").unwrap();
    let named = FuzzyRegex::new(r"(?P<n>abc){e<=1}").unwrap();
    for t in ["abc", "abx", "ab", "xabc", "abcd", "abcx", "yabcy"] {
        assert_eq!(span(&cap, t), span(&named, t), "cap vs named on {t:?}");
        assert_eq!(group1(&cap, t), group1(&named, t), "group on {t:?}");
    }
}

#[test]
fn capturing_group_over_repetition_and_alternation() {
    // Fuzzy over a quantified sub-pattern inside the capture (corpus L3786 shape).
    let re = FuzzyRegex::new(r"(x{6}){e<=1}").unwrap();
    assert_eq!(span(&re, "xxxxxx"), Some((0, 6))); // exact
    assert_eq!(span(&re, "xxxxx"), Some((0, 5))); // 1 deletion
    assert_eq!(group1(&re, "xxxxx").as_deref(), Some("xxxxx"));

    // Fuzzy over an alternation inside the capture.
    let alt = FuzzyRegex::new(r"(a|b){e<=1}").unwrap();
    assert_eq!(span(&alt, "a"), Some((0, 1)));
    assert_eq!(span(&alt, "c"), Some((0, 1))); // substitution
    assert_eq!(group1(&alt, "c").as_deref(), Some("c"));
}

#[test]
fn capturing_group_repetition_quantifier_still_works() {
    // `{2}` after a capturing group must remain a repetition, not fuzziness.
    let re = FuzzyRegex::new(r"(ab){2}").unwrap();
    assert_eq!(span(&re, "abab"), Some((0, 4)));
    assert!(re.find("ab").is_none());
    let range = FuzzyRegex::new(r"(ab){2,3}").unwrap();
    assert_eq!(span(&range, "ababab"), Some((0, 6)));
}

#[test]
fn capturing_group_individual_limits() {
    // Per-op fuzziness on a capturing group.
    let sub_only = FuzzyRegex::new(r"(abc){s<=1}").unwrap();
    assert_eq!(span(&sub_only, "abx"), Some((0, 3))); // 1 substitution ok
    assert!(sub_only.find("ab").is_none()); // deletion not allowed
}

#[test]
fn reverse_over_capturing_group_fuzzy() {
    // Corpus L3786: reverse + fuzzy over a capturing group.
    let re = FuzzyRegex::new(r"(?r)(x{6}){e<=1}").unwrap();
    assert_eq!(span(&re, "xxxxxx"), Some((0, 6)));
    assert_eq!(group1(&re, "xxxxxx").as_deref(), Some("xxxxxx"));
}
