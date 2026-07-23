//! Tests for coefficient-less cost expressions in fuzziness, e.g.
//! `{i+d+s<=N}`. These are the implicit-coefficient form of the weighted-cost
//! syntax (`{1i+1d+1s<=N}`) and cap the total weighted edit cost.

use fuzzy_regex::FuzzyRegex;

fn counts(re: &FuzzyRegex, text: &str) -> Option<(usize, usize, u32, u32, u32)> {
    re.find(text).map(|m| {
        let (s, i, d) = m.fuzzy_counts();
        (m.start(), m.end(), s, i, d)
    })
}

#[test]
fn cost_expression_compiles() {
    for pat in [
        r"(?:abc){i+d+s<=8}",
        r"(?:abc){s+i+d<=10}",
        r"(?:abc){i+d<=2}",
        r"(?:abc){i<=4,d<=4,s<=4,i+d+s<=8}",
        r"(?:abc){i+d+s+t<=4}",
    ] {
        assert!(FuzzyRegex::new(pat).is_ok(), "should compile: {pat}");
    }
}

#[test]
fn cost_expression_caps_total_edits() {
    let re = FuzzyRegex::new(r"^(?:abc){i+d+s<=2}$").unwrap();
    assert_eq!(counts(&re, "abc"), Some((0, 3, 0, 0, 0))); // exact
    assert_eq!(counts(&re, "abx"), Some((0, 3, 1, 0, 0))); // 1 substitution
    assert_eq!(counts(&re, "axx"), Some((0, 3, 2, 0, 0))); // 2 substitutions
    assert!(re.find("xxx").is_none()); // 3 substitutions exceed the cap
}

#[test]
fn implicit_matches_explicit_and_e_form() {
    // `i+d+s<=2` is equivalent to `1i+1d+1s<=2` and, for unit costs, to `e<=2`.
    let implicit = FuzzyRegex::new(r"^(?:abc){i+d+s<=2}$").unwrap();
    let explicit = FuzzyRegex::new(r"^(?:abc){1i+1d+1s<=2}$").unwrap();
    let e_form = FuzzyRegex::new(r"^(?:abc){e<=2}$").unwrap();
    for t in ["abc", "abx", "axx", "xxx", "ab", "abcd"] {
        assert_eq!(counts(&implicit, t), counts(&explicit, t), "on {t:?}");
        assert_eq!(counts(&implicit, t), counts(&e_form, t), "on {t:?}");
    }
}

#[test]
fn combined_per_op_and_cost_limits() {
    // Both the per-op caps and the total cost must hold.
    let re = FuzzyRegex::new(r"^(?:abcd){i<=1,d<=1,s<=2,i+d+s<=2}$").unwrap();
    assert_eq!(counts(&re, "abxd"), Some((0, 4, 1, 0, 0))); // 1 sub
    assert_eq!(counts(&re, "abxy"), Some((0, 4, 2, 0, 0))); // 2 subs (within s<=2, total<=2)
    assert!(re.find("xxxx").is_none()); // exceeds total cost
}

#[test]
fn weighted_coefficients_still_work() {
    // Explicit coefficients keep their weight.
    let re = FuzzyRegex::new(r"^(?:abc){2s<=2}$").unwrap();
    assert!(re.is_match("abx")); // 1 sub, cost 2 <= 2
    assert!(re.find("axx").is_none()); // 2 subs, cost 4 > 2
}
