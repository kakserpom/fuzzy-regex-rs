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

/// Multi-piece fuzzy patterns (2-3 fuzzy/literal pieces) that exercise `find()`'s
/// exact-shadow fast path (`try_exact_shadow` / `strip_fuzzy_to_exact`), which
/// the single-class strategy above never reaches.
///
/// Each pattern is produced together with its EXACT twin (the same pieces with
/// all fuzzy limits removed) so the test can use the exact pattern's own
/// leftmost-longest `find()` — a fully-trusted, bug-free oracle — instead of the
/// fuzzy `find_iter`, which has latent non-minimal divergences on these shapes.
fn multi_fuzzy_pair_strategy() -> impl Strategy<Value = (String, String)> {
    // Each piece is (fuzzy form, exact form).
    let piece = prop_oneof![
        (
            prop::sample::select(FUZZY_CLASSES),
            prop::sample::select(FUZZY_QUANTS),
            prop::sample::select(FUZZY_LIMITS),
        )
            .prop_map(|(c, q, l)| (format!("(?:{c}{q}){l}"), format!("{c}{q}"))),
        prop::sample::select(vec![" ", "-", ",", "@", "a", r"\d", "ab", "."])
            .prop_map(|s| (s.to_string(), s.to_string())),
        prop::sample::select(vec![
            ("(?:foo){e<=1}", "foo"),
            ("(?:ab){e<=2}", "ab"),
            (r"(?:\d+){e<=1}", r"\d+"),
        ])
        .prop_map(|(f, e)| (f.to_string(), e.to_string())),
    ];
    (
        any::<bool>(),
        prop::collection::vec(piece, 2..=3),
        any::<bool>(),
    )
        .prop_map(|(anchor_start, pieces, anchor_end)| {
            let (mut fuzzy, mut exact) = (String::new(), String::new());
            if anchor_start {
                fuzzy.push('^');
                exact.push('^');
            }
            for (f, e) in &pieces {
                fuzzy.push_str(f);
                exact.push_str(e);
            }
            if anchor_end {
                fuzzy.push('$');
                exact.push('$');
            }
            (fuzzy, exact)
        })
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

    /// The exact-shadow fast path must be correct WHENEVER IT FIRES (the only
    /// code path it changes). When the shadow fires it claims a 0-edit match at
    /// position 0, which — position 0 being the leftmost possible start and 0
    /// being the minimal edit count — is exactly the fuzzy pattern's leftmost
    /// result. We validate that against the EXACT twin's own leftmost-longest
    /// `find()`: the shadow's span must equal the exact pattern's match, and
    /// that match must start at 0. The exact twin has no fuzzy parts, so its
    /// `find()` is the trusted oracle (unlike the fuzzy `find_iter`, which is
    /// non-minimal on some of these shapes — a separate, pre-existing issue).
    #[test]
    fn exact_shadow_correct_when_it_fires(
        (fuzzy, exact) in multi_fuzzy_pair_strategy(),
        text in input_strategy(),
    ) {
        let Ok(fre) = FuzzyRegex::new(&fuzzy) else { return Ok(()); };
        let Ok(ere) = FuzzyRegex::new(&exact) else { return Ok(()); };

        let Some(shadow) = fre.debug_exact_shadow(&text) else { return Ok(()); };

        // The shadow only ever reports a genuine 0-edit match.
        prop_assert_eq!(
            shadow.fuzzy_counts(),
            (0, 0, 0),
            "shadow reported non-zero edits for [{}] on {:?}",
            fuzzy,
            text
        );

        // The exact twin's leftmost-longest match is the trusted oracle: the
        // shadow fired, so an exact match exists at 0, so the twin's leftmost
        // match must be at 0 with the same span the shadow reports.
        let exact_span = ere.find(&text).map(|m| (m.start(), m.end()));
        prop_assert_eq!(
            Some((shadow.start(), shadow.end())),
            exact_span,
            "shadow [{}] on {:?} disagrees with exact twin [{}]",
            fuzzy,
            text,
            exact
        );

        // find() must return exactly the shadow's result (it is tried first).
        let find_span = fre.find(&text).map(|m| (m.start(), m.end()));
        prop_assert_eq!(
            find_span,
            Some((shadow.start(), shadow.end())),
            "find() did not use the shadow result for [{}] on {:?}",
            fuzzy,
            text
        );
    }
}
