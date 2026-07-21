//! Regression tests for `find()` vs `find_iter()` consistency on DFA-backed
//! patterns.
//!
//! Primary target: the DFA `find`/`find_all` discrepancy. `Dfa::find` uses a
//! prefilter scan (`find_with_prefilter`) while `Dfa::find_all` loops `find_at`.
//! The prefilter path used to fall back to a bogus empty match at position 0
//! for end-anchored, empty-accepting patterns (e.g. `,?$`), ignoring the end
//! anchor — disagreeing with `find_all`/`find_iter`.
//!
//! Also covers two fast-path hijacks fixed alongside it: the end-anchored exact
//! literal path (`[0-9]{2}(?:,\d)$`) and the bounded dot-repeat mis-detected as
//! `.*` (`^.{1,3}$`).

use fuzzy_regex::FuzzyRegex;

fn check(pat: &str, input: &str, expected: Option<(usize, usize)>) {
    let re = FuzzyRegex::new(pat).unwrap();
    let f = re.find(input).map(|m| (m.start(), m.end()));
    let it = re.find_iter(input).next().map(|m| (m.start(), m.end()));
    assert_eq!(f, it, "find != find_iter.next for [{pat}] on {input:?}");
    assert_eq!(f, expected, "wrong span for [{pat}] on {input:?}");
}

#[test]
fn end_anchored_optional_empty_match_at_end() {
    // `,?$`: empty match must sit at end-of-text, not position 0.
    check(r",?$", "b.,a", Some((4, 4)));
    check(r",?$", "-", Some((1, 1)));
    check(r",?$", "", Some((0, 0)));
    check(r",?$", ",", Some((0, 1)));
    check(r"x?$", "abc", Some((3, 3)));
}

#[test]
fn end_anchored_no_bogus_zero_length_match() {
    // Patterns that should NOT match must not yield a spurious (0,0).
    check(r"\d,$", "abc", None);
    check(r"[0-9]{2}(?:,\d)$", "$3  a,", None);
    check(r"[0-9]{2}(?:,\d)$", "32-- , ,", None);
}

#[test]
fn end_anchored_literal_not_hijacked_by_structure() {
    // `[0-9]{2},$` has a real match; the single-literal `rfind` fast path must
    // not fire (it would match the trailing comma alone).
    check(r"[0-9]{2},$", "ab12,", Some((2, 5)));
    // Pure end-anchored literals must still work (fast path stays valid).
    check(r"foo$", "a foo", Some((2, 5)));
    check(r"foo$", "foobar", None);
    check(r"hello$", "say hello", Some((4, 9)));
    // Start anchor must be honored (rfind ignores `^`).
    check(r"^foo$", "xfoo", None);
    check(r"^foo$", "foo", Some((0, 3)));
}

#[test]
fn bounded_dot_repeat_not_treated_as_dotstar() {
    check(r"^.{1,3}$", "", None);
    check(r"^.{1,3}$", "ab", Some((0, 2)));
    check(r"^.{1,3}$", "abc", Some((0, 3)));
    check(r"^.{1,3}$", "abcd", None);
    // Genuine dot-star still matches the whole text.
    check(r"^.*$", "abcd", Some((0, 4)));
    check(r"^.*$", "", Some((0, 0)));
}

#[test]
fn end_anchor_after_group_detected() {
    // `$` after an optional/repeated/alternation group must be recognized as an
    // end anchor, so the memchr fast path does not treat these as bare literals.
    // Convergent branches (both arms reach the same `$`):
    check(r"(?:ab)?$", "1,b", Some((3, 3)));
    check(r"(?:ab)?$", "", Some((0, 0)));
    check(r"(?:ab)?$", "xab", Some((1, 3)));
    check(r"(?:ab|cd)$", "xcd", Some((1, 3)));
    check(r"(?:ab|cd)$", "xef", None);
    // Looping groups (cycle in the NFA) still end with `$`:
    check(r"(?:ab)*$", "1,b", Some((3, 3)));
    check(r"(?:ab)*$", "abab", Some((0, 4)));
    check(r"(?:ab)+$", "abab", Some((0, 4)));
    check(r"(?:ab)+$", "xab", Some((1, 3)));
}

#[test]
fn empty_accepting_class_finds_leftmost_not_empty_end() {
    // Empty-accepting patterns must not fall back to a prefilter that keys on a
    // single representative byte (e.g. `[0-9]?` -> `0`), which missed the real
    // leftmost match and returned the empty end match instead.
    check(r"[0-9]?$", "2.2", Some((2, 3)));
    check(r"[0-9]?$", "22", Some((1, 2)));
    check(r"[0-9]?$", "abc", Some((3, 3))); // no digit -> empty at end
    check(r"[0-9]?$", "", Some((0, 0)));
    check(r"[0-9]*$", "2.2", Some((2, 3)));
    check(r"[a-c]?$", "zzb", Some((2, 3)));
    check(r"[a-c]*$", "zzb", Some((2, 3)));
}

#[test]
fn optional_leading_element_prefilter_is_sound() {
    // A `$`-less pattern with an OPTIONAL leading element followed by a
    // mandatory one: the prefilter used to key only on the optional element's
    // byte (e.g. `,` for `,?\d`) and miss matches that begin with the mandatory
    // element. The prefilter must now cover every possible first byte.
    check(r",?\d", "23b2 1", Some((0, 1)));
    check(r",?\d", "33b,", Some((0, 1)));
    check(r",?\d", ",5", Some((0, 2)));
    check(r",?\d", "x,5", Some((1, 3)));
    check(r"-?\d+", "abc42", Some((3, 5)));
    check(r"[+-]?\d+", "x-9", Some((1, 3)));
    check(r"\.?[a-z]", "12.b", Some((2, 4)));
    check(r"-?\d+$", "x-42", Some((1, 4)));
}

#[test]
fn class_plus_with_literal_optional_leading() {
    // `find` used a dedicated helper that extended by a fixed word/email
    // charset regardless of the actual class, so non-word classes before a
    // literal mis-matched. `find` now delegates to the same class-aware logic
    // `find_iter` uses.
    check(r"\d?,", ", bb,b+-", Some((0, 1)));
    check(r"\d?,", "b,.2-1", Some((1, 2)));
    check(r"\d?,", "23,", Some((1, 3)));
    check(r"[+-]?\.\d", "2.b  ,a", None);
    check(r"\d?a[0-9]", "a2ba3", Some((0, 2)));
    check(r"\d?a[0-9]", ",+aaaa1", Some((5, 7)));
    // Genuine email/word patterns still match via the fast path.
    check(r"\w+@\w+", "hi a@b x", Some((3, 6)));
    check(r"[\w.]+@[\w.]+", "to x.y@z.com!", Some((3, 12)));
}

#[test]
fn literal_plus_before_class_not_treated_as_bare_class_plus() {
    // A leading literal-plus followed by a class (`\.+\d`, `-+\d`, `@+\d{2}`)
    // used to be mis-detected as a bare character-class-plus, so the DFA's
    // `find_char_class_plus` matched only the trailing digit run.
    check(r"\.+\d", "x..5", Some((1, 4)));
    check(r"-+\d", "a--5", Some((1, 4)));
    check(r"@+\d{2}", "@2312a13-2", Some((0, 3)));
    check(r"@+\d{2}", "b1.1.2@", None);
}

#[test]
fn consecutive_unbounded_quantifiers_compile_without_overflow() {
    // Patterns with consecutive `.`-quantifiers create cyclic NFAs. The
    // greedy-prefix-with-suffix detector recursed without cycle detection and
    // overflowed the stack at compile time. These must now compile and match
    // consistently.
    for pat in [
        r".+.+",
        r".*.*",
        r".+.+.+",
        r".+[+-]+",
        r"^.+a{1,3}[+-]+$",
        r"(?:.+)+",
    ] {
        let re =
            FuzzyRegex::new(pat).unwrap_or_else(|e| panic!("[{pat}] failed to compile: {e:?}"));
        // Also exercise matching to be sure the runtime paths don't recurse.
        let _ = re.find("abcd-+xy");
        let _ = re.find_iter("abcd-+xy").next();
    }
    // Genuine `.*SUFFIX` / `.+SUFFIX` still match via the fast path.
    check(r".*foo", "xx foo yy", Some((0, 6)));
    check(r".+end", "the end", Some((0, 7)));
    check(r".*\.txt", "a/b/c.txt", Some((0, 9)));
}

#[test]
fn genuine_char_class_plus_still_matches() {
    // The single-class-plus fast path must keep working.
    check(r"\d+", "ab123c", Some((2, 5)));
    check(r"\w+", "  hello_9 x", Some((2, 9)));
    check(r"[a-z]+", "AB cde F", Some((3, 6)));
    check(r"\d+", "", None);
    check(r"\d{4}-\d{2}-\d{2}", "d 2020-01-02 x", Some((2, 12)));
    check(r"\d+\.\d+", "pi=3.14!", Some((3, 7)));
}

#[test]
fn alternation_prefilter_still_works() {
    // The Split-branch prefilter must keep working for genuine alternations.
    check(r"(?:a|b)c", "xbc", Some((1, 3)));
    check(r"(?:foo|bar)", "x bar", Some((2, 5)));
    check(r"(?:cat|dog|fish)", "a fish here", Some((2, 6)));
}

#[test]
fn greedy_prefix_requires_pure_single_literal_suffix() {
    // The `.*SUFFIX` fast path treats `literals[0]` as the ENTIRE suffix. It must
    // only fire when the suffix is exactly one fixed literal.
    // Structured suffix (class / group after the literal):
    check(r".+@.{2}", "b1.1.2@", None); // needs 2 chars after '@'
    check(r".+-\w{2}", "x-ab", Some((0, 4)));
    check(r".+\.(?:a|b)", "z.a", Some((0, 3)));
    // Multi-segment literal suffix (`literals[0]` is only the first piece):
    check(r"^.+-(?:ab)", "x-ab", Some((0, 4)));
    check(r"^.+aa{2}", "..21ab2 ", None); // no "aaa"
    check(r"^.+aa{2}", "xaaaa", Some((0, 5)));
    check(r"^.+(?:ab){2}", "xababab", Some((0, 7)));
    // Genuine single-literal suffixes still use the fast path (correctly):
    check(r".+foo", "xx foo yy", Some((0, 6)));
    check(r".+\.txt", "a/b/c.txt", Some((0, 9)));
    check(r".+a", "xya", Some((0, 3)));
    check(r".+ab", "xyab", Some((0, 4)));
    check(r".*bar", "a bar b", Some((0, 5)));
}

#[test]
fn find_matches_find_iter_first_for_fuzzy() {
    // Regressions caught by the debug consistency guard (find == find_iter.next):
    // 1. search_first's "exact substring anywhere" shortcut returned a later
    //    exact match over an earlier fuzzy one.
    check(
        r"(?:test){e<=1}",
        "best tset trial test contest",
        Some((0, 4)),
    );
    // 2. Per-op-limit patterns went through an NFA fallback that returned
    //    overlapping, unsorted matches.
    check(r"(?:hello){t<=1,e<=2}", "hello", Some((0, 5)));
    // 3. Char-class edit restrictions: the first Bitap match could fail the
    //    restriction while a later one passed; find must skip to the passing one.
    check(r"(?:vb){s<=1:[0-9]}", " bav5", {
        let re = FuzzyRegex::new(r"(?:vb){s<=1:[0-9]}").unwrap();
        re.find_iter(" bav5").next().map(|m| (m.start(), m.end()))
    });
}

#[test]
fn digit_sequence_requires_all_digit_classes() {
    // A non-digit class between digits and the separator (`\d{1,3}?[a-z]\.`) is
    // not a digit-sequence-with-separator; the date fast path must not claim it.
    // `",,."` cannot match `\d[a-z]\.` with zero edits (no digit, no letter), so
    // the answer is None — the old `find_iter` reported a spurious `(0,3)` here.
    check(r"\d{1,3}?[a-z]\.", ",,. -b.", None);
    check(r"\d{1,3}?[a-z]\.", "12b.", Some((0, 4)));
    check(r"\d+[a-z]\.", "5x.", Some((0, 3)));
    // Genuine digit-only sequences with separators still use the fast path.
    check(r"\d{4}-\d{2}-\d{2}", "d 2020-01-02 x", Some((2, 12)));
    check(r"\d{3}-\d{4}", "call 555-1234", Some((5, 13)));
    check(r"\d+\.\d+", "pi=3.14!", Some((3, 7)));
    check(r"\d{1,3}\.\d{1,3}", "ip 10.0", Some((3, 7)));
}

#[test]
fn dot_then_repeated_group_not_treated_as_dotstar_prefix() {
    // `^.(?:,\d)*` is a SINGLE dot followed by a repeated group `(?:,\d)*`, not
    // `.*SUFFIX`. The greedy-prefix-with-suffix detector must require the `*`/`+`
    // to repeat the dot itself (a Split branch looping back to it).
    check(r"^.(?:,\d)*", "3-", Some((0, 1)));
    check(r"^.(?:,\d)*", "5,1,2x", Some((0, 5)));
    check(r".(?:,\d)*", "a,1,2", Some((0, 5)));
    // Genuine `.*SUFFIX` / `.+SUFFIX` still detected + correct.
    check(r".*foo", "xx foo yy", Some((0, 6)));
    check(r".*\.txt", "a/b/c.txt", Some((0, 9)));
    check(r".+bar", "x bar", Some((0, 5)));
    check(r"^.*x$", "aaax", Some((0, 4)));
}

#[test]
fn dot_plus_does_not_match_empty_text() {
    // `.+` needs at least one char, so it must NOT match empty text (the
    // "match whole text" fast path used to return (0,0)). `.*` still matches
    // empty; both match the whole non-empty text.
    check(r".+", "", None);
    check(r"^.+", "", None);
    check(r"^.+$", "", None);
    check(r".+", "abc", Some((0, 3)));
    check(r"^.+$", "xy", Some((0, 2)));
    check(r".*", "", Some((0, 0)));
    check(r"^.*$", "", Some((0, 0)));
    check(r".*", "abc", Some((0, 3)));
    // min>=2 dot-repeat: a too-short text must not match either.
    check(r".{2,}", "a", None);
    check(r".{2,}", "abc", Some((0, 3)));
    check(r".{2,}", "", None);
}

#[test]
fn quantified_literal_group_not_treated_as_fixed_literal() {
    // The memchr fast path treats the pattern as a single FIXED literal. A
    // quantified/optional literal group (`(?:ab)*`, `(?:ab)?`, `(?:ab)+`) is
    // not fixed — the empty-accepting forms must match empty, not return None.
    check(r"(?:ab)*", " 3b-,@a2", Some((0, 0)));
    check(r"(?:ab)*", "xababy", Some((0, 0)));
    check(r"(?:ab)?", " 3b-,@a2", Some((0, 0)));
    check(r"(?:ab)?", "abc", Some((0, 2)));
    check(r"(?:ab)+", "xabab", Some((1, 5)));
    check(r"(?:ab)+", "xyz", None);
    // Plain fixed literals still use the fast path (fast + correct).
    check(r"foo", "a foo b", Some((2, 5)));
    check(r"ab", "xabab", Some((1, 3)));
    check(r"hello", "say hello", Some((4, 9)));
    check(r"foo", "no match", None);
}

#[test]
fn empty_accepting_greedy_still_matches_fully() {
    // Greedy empty-accepting patterns must still match the full run at the
    // leftmost position (find_at returns the longest match there).
    check(r"\w*", "abc", Some((0, 3)));
    check(r"\d*", "12x", Some((0, 2)));
    check(r"[0-9]*", "12x", Some((0, 2)));
    check(r"a*", "bbb", Some((0, 0)));
    check(r"[a-c]*", "xy", Some((0, 0)));
}

#[test]
fn lazy_char_class_plus_uses_correct_class_matcher() {
    // The `find()` char-class-plus fast path defaulted to a WORD matcher when the
    // class had no named type (custom ranges like `[a-z]`, or a literal char like
    // `a`), so it matched the wrong bytes (`a+?` matched "x"; `[a-z]+?` matched a
    // digit). `find()` now falls through to the class-aware engine for those,
    // exactly like `find_iter()` — only known named classes take the byte fast path.
    check(r"a+?", "xaay", Some((1, 2)));
    check(r"[a-z]+?", "12ab", Some((2, 3)));
    check(r"[a-c]+?", "z b aa", Some((2, 3)));
    check(r"[+-]+?", "x+-", Some((1, 2)));
    // Greedy custom-range/literal plus must also be class-aware.
    check(r"[a-z]+", "AB cde F", Some((3, 6)));
    check(r"[a-c]+", "z b aa", Some((2, 3)));
    check(r"a+", "xaay", Some((1, 3)));
    // Genuine named-class plus still uses the fast byte path (fast + correct).
    check(r"\d+?", "x12", Some((1, 2)));
    check(r"\d+", "x12", Some((1, 3)));
    check(r"\w+", "  ab_c ", Some((2, 6)));
}

#[test]
fn greedy_prefix_honors_leading_repeat_minimum() {
    // `.+SUFFIX` requires at least one char before the suffix. When the only
    // suffix occurrence sits at position 0 the greedy prefix cannot meet its
    // minimum, so there is no match — the fast path used to return a bogus
    // (0, len) anchored at 0.
    check(r".+@", "@13", None);
    check(r".+-", "-1@@", None);
    check(r".+@", "x@y", Some((0, 2)));
    check(r".+end", "aend", Some((0, 4)));
    // `.*SUFFIX` (min 0) still matches with the suffix at position 0.
    check(r".*foo", "fooxx", Some((0, 3)));
    check(r".*@", "@ab", Some((0, 1)));
}

#[test]
fn digit_sequence_requires_trailing_digit_group() {
    // A TRAILING separator (`\d{1,3}?\.` = digits then `.` with nothing after) is
    // not a digit-sequence date; the date fast path mishandled it (matched ".1"
    // on ".1 "). A genuine sequence has the separator strictly BETWEEN digits.
    check(r"\d{1,3}?\.", ".1   ", None);
    check(r"\d+\.", "ab12.", Some((2, 5)));
    // Genuine digit-sequences (separator between digit groups) are unaffected.
    check(r"\d+\.\d+", "pi=3.14", Some((3, 7)));
    check(r"\d{4}-\d{2}-\d{2}", "d 2020-01-02", Some((2, 12)));
    check(r"\d{1,3}\.\d{1,3}", "ip 10.0", Some((3, 7)));
}
