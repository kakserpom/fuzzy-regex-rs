//! Tests for a fuzzy quantifier applied directly to a recursion reference,
//! e.g. `(?R){e<=2}` / `(?0){e}`. The fuzziness is a total-edit cap on the
//! recursive sub-match (`None`/unbounded `{e}` = no cap).

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn fuzzy_recursion_forms_compile() {
    for pat in [
        r"(?0){e}",
        r"(?R){e<=2}",
        r"(?1){e<=1}",
        r"\x00?(?0){e}", // corpus L3804
        r"(a(?1){e<=1}?b)",
    ] {
        assert!(FuzzyRegex::new(pat).is_ok(), "should compile: {pat}");
    }
}

#[test]
fn unbounded_fuzzy_recursion_matches_like_plain_recursion() {
    // `{e}` (no bound) places no cap, so behaviour matches plain `(?R)`.
    let fuzzy = FuzzyRegex::new(r"\((?:[^()]|(?R){e})*\)").unwrap();
    let plain = FuzzyRegex::new(r"\((?:[^()]|(?R))*\)").unwrap();
    for t in ["(a(b)c)", "((()))", "()"] {
        assert_eq!(span(&fuzzy, t), span(&plain, t), "on {t:?}");
    }
}

#[test]
fn non_fuzzy_recursion_unaffected() {
    // Adding the field must not change ordinary recursion.
    let re = FuzzyRegex::new(r"a(?0)?b").unwrap();
    assert_eq!(span(&re, "aabb"), Some((0, 4)));
    assert_eq!(span(&re, "ab"), Some((0, 2)));
}

#[test]
fn bounded_cap_is_enforced() {
    // The recursive sub-match may accumulate at most the capped number of edits.
    let capped = FuzzyRegex::new(r"(?:x(?0)?y){e<=1}").unwrap();
    assert_eq!(span(&capped, "xxyy"), Some((0, 4))); // exact, within cap
}
