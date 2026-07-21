//! Tests for `MatchEndPolicy` — switching between longest-within-budget and
//! minimum-edit end selection for fuzzy matches.

use fuzzy_regex::{FuzzyRegexBuilder, MatchEndPolicy};

fn build(pat: &str, policy: MatchEndPolicy) -> fuzzy_regex::FuzzyRegex {
    FuzzyRegexBuilder::new(pat)
        .match_end_policy(policy)
        .build()
        .unwrap()
}

fn span(m: Option<fuzzy_regex::Match<'_>>) -> Option<(usize, usize)> {
    m.map(|m| (m.start(), m.end()))
}

#[test]
fn default_policy_is_longest_within_budget() {
    // Default builder (no policy set) keeps the historical longest-span behavior.
    let re = FuzzyRegexBuilder::new(r"(?:error){e}").build().unwrap();
    assert_eq!(span(re.find("regex failure")), Some((0, 8)));
}

#[test]
fn longest_within_budget_explicit() {
    let re = build(r"(?:error){e}", MatchEndPolicy::LongestWithinBudget);
    assert_eq!(span(re.find("regex failure")), Some((0, 8)));
}

#[test]
fn min_edit_tightens_unlimited_span() {
    // Unlimited {e}: MinEdit reports the tight, pattern-length alignment "regex"
    // rather than the widest span the budget allows.
    let re = build(r"(?:error){e}", MatchEndPolicy::MinEdit);
    assert_eq!(span(re.find("regex failure")), Some((0, 5)));
}

#[test]
fn min_edit_tightens_bounded_span() {
    // Same effect with a large bounded budget.
    let re = build(r"(?:error){e<=4}", MatchEndPolicy::MinEdit);
    assert_eq!(span(re.find("regex failure")), Some((0, 5)));
}

#[test]
fn find_find_iter_captures_agree_under_min_edit() {
    let re = build(r"(?:error){e}", MatchEndPolicy::MinEdit);
    let text = "regex failure";
    let f = span(re.find(text));
    let first_iter = span(re.find_iter(text).next());
    let cap = re
        .captures(text)
        .and_then(|c| c.get(0).map(|m| (m.start(), m.end())));
    assert_eq!(f, Some((0, 5)));
    assert_eq!(first_iter, Some((0, 5)));
    assert_eq!(cap, Some((0, 5)));
}

#[test]
fn min_edit_does_not_change_exact_or_natural_matches() {
    // When the natural alignment is already tight, both policies agree.
    for policy in [MatchEndPolicy::LongestWithinBudget, MatchEndPolicy::MinEdit] {
        let re = build(r"(?:hello){e}", policy);
        assert_eq!(
            span(re.find("hello world")),
            Some((0, 5)),
            "policy {policy:?}"
        );

        let re = build(r"(?:hello){e<=2}", policy);
        assert_eq!(
            span(re.find("hxllo there")),
            Some((0, 5)),
            "policy {policy:?}"
        );

        let re = build(r"(?:cat){e}", policy);
        assert_eq!(
            span(re.find("the category")),
            Some((4, 7)),
            "policy {policy:?}"
        );
    }
}

#[test]
fn min_edit_never_extends_beyond_longest() {
    // MinEdit should always report a span that is a subset of (or equal to) the
    // longest-within-budget span for the same start.
    let cases = [
        (r"(?:error){e}", "regex failure"),
        (r"(?:error){e<=4}", "regex failure"),
        (r"(?:hello){e}", "hello world"),
        (r"(?:cat){e}", "the category"),
    ];
    for (pat, text) in cases {
        let longest = span(build(pat, MatchEndPolicy::LongestWithinBudget).find(text));
        let min_edit = span(build(pat, MatchEndPolicy::MinEdit).find(text));
        if let (Some((ls, le)), Some((ms, me))) = (longest, min_edit) {
            assert_eq!(ls, ms, "{pat} on {text}: start should match");
            assert!(
                me <= le,
                "{pat} on {text}: min-edit end {me} should not exceed longest end {le}"
            );
        }
    }
}
