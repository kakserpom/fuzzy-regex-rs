//! Property-based test locking in `find(text) == find_iter(text).next()`.
//!
//! `find()` carries ~15 heuristic "shape" fast-path dispatch blocks; `find_iter()`
//! has its own. Historically these drifted apart — a fast path would grab a
//! pattern it could not handle and return a truncated, missing, or spurious span
//! that disagreed with the (correct) `find_iter()` result. A long sequence of
//! such divergences was enumerated and fixed (see CHANGELOG "Fixed"); this test
//! generates patterns from the same atom/quantifier grammar that surfaced them
//! and asserts the two entry points agree, so any regression is caught.
//!
//! Scope: default (leftmost) matching only. `BESTMATCH`/`ENHANCEMATCH`/`POSIX`
//! and recursive patterns legitimately differ and are out of scope here.
//!
//! Run with: cargo test --test find_iter_consistency_proptest

use fuzzy_regex::FuzzyRegex;
use proptest::prelude::*;

/// Atoms known to exercise the fast-path dispatch: named classes, custom ranges,
/// literals, the dot, non-capturing groups/alternations, bounded/lazy repeats,
/// and the separator characters that drive the currency/date/class-plus paths.
const ATOMS: &[&str] = &[
    r"\d",
    r"[a-z]",
    r"\w",
    "a",
    ",",
    ".",
    "-",
    r"\.",
    r"(?:,\d)",
    r"(?:ab)",
    r"\d{1,3}",
    r"[0-9]",
    r".",
    r"[a-c]",
    r"(?:a|b)",
    "@",
    r"[+-]",
    r"\d+?",
    r"[a-z]+?",
    r"\w+",
    r"[^0-9]",
    r"\s",
    "b",
    r"(?:a|bc)",
    r"\d{2}",
];

/// Quantifiers: greedy/lazy `*`/`+`/`?`, fixed and bounded counts, and an
/// unbounded lower bound (`{2,}`).
const QUANTS: &[&str] = &[
    "", "*", "+", "?", "{2}", "{1,3}", "*?", "+?", "??", "{1,3}?", "{2,}",
];

/// The small input alphabet (all ASCII): letters, digits, and the punctuation
/// the atoms care about, plus space and underscore.
const ALPHABET: &[u8] = b"abc123,.-@ _";

/// Strategy producing a pattern string from the grammar: optional `^`, then 1-4
/// `atom + quant` pieces, then optional `$`.
fn pattern_strategy() -> impl Strategy<Value = String> {
    (
        any::<bool>(),
        prop::collection::vec(
            (prop::sample::select(ATOMS), prop::sample::select(QUANTS)),
            1..=4,
        ),
        any::<bool>(),
    )
        .prop_map(|(anchor_start, pieces, anchor_end)| {
            let mut pat = String::new();
            if anchor_start {
                pat.push('^');
            }
            for (atom, quant) in pieces {
                pat.push_str(atom);
                pat.push_str(quant);
            }
            if anchor_end {
                pat.push('$');
            }
            pat
        })
}

/// Strategy producing an input string over the small ASCII alphabet (0-14 chars).
fn input_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(ALPHABET), 0..=14)
        .prop_map(|bytes| String::from_utf8(bytes).expect("ALPHABET is ASCII"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// `find` must return exactly the first match `find_iter` yields.
    #[test]
    fn find_agrees_with_find_iter_first(
        pattern in pattern_strategy(),
        text in input_strategy(),
    ) {
        // Skip patterns that fail to compile (some atom/quant combinations are
        // not valid regex, e.g. a leading `*`); they are not interesting here.
        let Ok(re) = FuzzyRegex::new(&pattern) else { return Ok(()); };

        let find_span = re.find(&text).map(|m| (m.start(), m.end()));
        let iter_span = re.find_iter(&text).next().map(|m| (m.start(), m.end()));

        prop_assert_eq!(
            find_span,
            iter_span,
            "find() disagrees with find_iter().next() for [{}] on {:?}",
            pattern,
            text
        );
    }
}

/// Char classes used by the fuzzy-class-plus fast path (`find_dispatch`'s
/// `(?:CLASS+){e<=k}` branch, backed by `Nfa::fuzzy_char_class_plus`).
const FUZZY_CLASSES: &[&str] = &[r"\w", r"[a-z]", r"\d", r"[a-c]", r"\s", r"[^0-9]"];
/// Repetition + fuzzy-limit suffixes that produce a single fuzzy class-plus.
const FUZZY_QUANTS: &[&str] = &["+", "*"];
const FUZZY_LIMITS: &[&str] = &[
    "{e<=1}",
    "{e<=2}",
    "{i<=1}",
    "{d<=1}",
    "{s<=1}",
    "{i<=1,d<=1,s<=1}",
    "{i<=2}",
    "{s<=2}",
];

fn fuzzy_class_plus_strategy() -> impl Strategy<Value = String> {
    (
        prop::sample::select(FUZZY_CLASSES),
        prop::sample::select(FUZZY_QUANTS),
        prop::sample::select(FUZZY_LIMITS),
    )
        .prop_map(|(class, quant, limit)| format!("(?:{class}{quant}){limit}"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20000))]

    /// The `find()` 0-edit fast path for `(?:CLASS+){fuzzy}` must return exactly
    /// what the general `find_iter()` NFA path returns — same span AND same edit
    /// counts (the fast path claims 0 edits, so a divergence in the (s,i,d) tuple
    /// is as much a bug as a wrong span).
    #[test]
    fn fuzzy_class_plus_fast_path_matches_general(
        pattern in fuzzy_class_plus_strategy(),
        text in input_strategy(),
    ) {
        let Ok(re) = FuzzyRegex::new(&pattern) else { return Ok(()); };

        let find_m = re.find(&text).map(|m| (m.start(), m.end(), m.fuzzy_counts()));
        let iter_m = re.find_iter(&text).next().map(|m| (m.start(), m.end(), m.fuzzy_counts()));

        prop_assert_eq!(
            find_m,
            iter_m,
            "fuzzy-class-plus fast path disagrees with find_iter().next() for [{}] on {:?}",
            pattern,
            text
        );
    }
}
