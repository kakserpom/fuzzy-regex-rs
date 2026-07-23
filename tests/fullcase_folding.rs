//! Tests for the `(?f)` full case-folding flag (bidirectional 1↔N folding:
//! `ß` ↔ "ss", ligatures ↔ their expansions). Only effective with `(?i)`.

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn forward_sharp_s_matches_expansion() {
    let re = FuzzyRegex::new(r"(?fi)\N{LATIN SMALL LETTER SHARP S}").unwrap();
    assert_eq!(span(&re, "ss"), Some((0, 2)));
    assert_eq!(span(&re, "SS"), Some((0, 2)));
    assert_eq!(span(&re, "ß"), Some((0, 2))); // ß is 2 bytes
}

#[test]
fn forward_does_not_match_partial_expansion() {
    // ß folds to "ss"; a single "s" must NOT match (no half-expansion).
    let re = FuzzyRegex::new(r"(?fi)\N{LATIN SMALL LETTER SHARP S}").unwrap();
    assert!(re.find("s").is_none());
}

#[test]
fn reverse_expansion_matches_single_char() {
    let re = FuzzyRegex::new(r"(?fi)ss").unwrap();
    assert_eq!(span(&re, "ß"), Some((0, 2)));
    assert_eq!(span(&re, "ss"), Some((0, 2)));
    assert_eq!(span(&re, "SS"), Some((0, 2)));
}

#[test]
fn reverse_within_a_word() {
    let re = FuzzyRegex::new(r"(?fi)mass").unwrap();
    assert_eq!(span(&re, "maß"), Some((0, 4))); // "ma" + ß(2 bytes)
    assert_eq!(span(&re, "mass"), Some((0, 4)));
}

#[test]
fn ligatures_fold_both_ways() {
    let ff = FuzzyRegex::new(r"(?fi)\N{LATIN SMALL LIGATURE FF}").unwrap();
    assert_eq!(span(&ff, "ff"), Some((0, 2)));
    let ff_rev = FuzzyRegex::new(r"(?fi)ff").unwrap();
    assert_eq!(span(&ff_rev, "ﬀ"), Some((0, 3))); // ﬀ is 3 bytes
}

#[test]
fn only_active_with_ignorecase() {
    // (?f) alone (no (?i)) does not fold.
    assert!(FuzzyRegex::new(r"(?f)ss").unwrap().find("ß").is_none());
    // (?i) alone (no (?f)) does not fold ß to "ss".
    assert!(
        FuzzyRegex::new(r"(?i)\N{LATIN SMALL LETTER SHARP S}")
            .unwrap()
            .find("ss")
            .is_none()
    );
}

#[test]
fn non_folding_literals_are_unchanged() {
    // A literal with no fold-expanding content behaves exactly like `(?i)`.
    let f = FuzzyRegex::new(r"(?fi)error").unwrap();
    assert_eq!(span(&f, "ERROR"), Some((0, 5)));
    assert!(f.find("erro").is_none());
    // Fuzzy over a non-folding literal keeps its edit budget.
    let fuzzy = FuzzyRegex::new(r"(?fi)(?:error){e<=1}").unwrap();
    assert_eq!(span(&fuzzy, "errar"), Some((0, 5))); // 1 substitution
    assert!(fuzzy.find("xxxxx").is_none());
}

#[test]
fn via_builder() {
    use fuzzy_regex::FuzzyRegexBuilder;
    let re = FuzzyRegexBuilder::new(r"\N{LATIN SMALL LETTER SHARP S}")
        .case_insensitive(true)
        .fullcase(true)
        .build()
        .unwrap();
    assert_eq!(span(&re, "ss"), Some((0, 2)));
    assert_eq!(span(&re, "ß"), Some((0, 2)));
}

#[test]
fn fuzzy_over_folded_char() {
    // Corpus L4448 shape: fuzzy over ß with an edit-char restriction.
    let re = FuzzyRegex::new(r"(?fiu)(?:\N{LATIN SMALL LETTER SHARP S}){e<=1:[a-z]}").unwrap();
    assert_eq!(span(&re, "ss"), Some((0, 2))); // exact fold
    assert_eq!(span(&re, "ts"), Some((0, 2))); // 1 substitution from "ss"
    assert_eq!(span(&re, "ß"), Some((0, 2))); // the char itself
}
