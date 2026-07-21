//! Fuzz tests for fuzzy-regex
//!
//! Run with: cargo test --test fuzz

use fuzzy_regex::{FuzzyRegex, FuzzyRegexBuilder};

/// Fuzz test: compile various patterns
#[test]
fn fuzz_compile() {
    let patterns = [
        "hello",
        "(?:hello){e<=1}",
        "(?:hello){e<=2}",
        "[a-z]+",
        "\\d+",
        "\\w+",
        "(?:a){e<=1}",
        "(?:ab){e<=1}",
        "(?:test){e<=1}",
        "(?m)^hello$",
        "(?:hello|world)",
    ];

    for pattern in patterns {
        let _ = FuzzyRegex::new(pattern);
    }
}

/// Fuzz test: is_match with random inputs
#[test]
fn fuzz_is_match() {
    let patterns = [
        "hello",
        "(?:hello){e<=1}",
        "(?:hello){e<=2}",
        "[a-z]+",
        "\\d+",
        "(?:test){e<=1}",
    ];

    let texts = [
        "hello",
        "helo",
        "helloo",
        "hallo",
        "world",
        "test",
        "",
        "   ",
        "hello world",
    ];

    for pattern in patterns {
        if let Ok(re) = FuzzyRegex::new(pattern) {
            for text in texts {
                let _ = re.is_match(text);
            }
        }
    }
}

/// Fuzz test: find with random inputs
#[test]
fn fuzz_find() {
    let patterns = ["hello", "(?:hello){e<=1}", "(?:test){e<=1}", "[a-z]+"];

    let texts = ["hello", "helo", "test string", "", "   hello   "];

    for pattern in patterns {
        if let Ok(re) = FuzzyRegex::new(pattern) {
            for text in texts {
                let _ = re.find(text);
                let _ = re.find_iter(text).collect::<Vec<_>>();
            }
        }
    }
}

/// Fuzz test: capture groups
#[test]
fn fuzz_captures() {
    let patterns = ["(hello)", "(\\w+)@(\\w+)", "(?P<name>\\w+)"];

    let texts = ["hello", "test@example", ""];

    for pattern in patterns {
        if let Ok(re) = FuzzyRegex::new(pattern) {
            for text in texts {
                let _ = re.captures(text);
            }
        }
    }
}

/// Fuzz test: builder options
#[test]
fn fuzz_builder() {
    let options = vec![
        FuzzyRegexBuilder::new("test"),
        FuzzyRegexBuilder::new("test").case_insensitive(true),
        FuzzyRegexBuilder::new("test").multi_line(true),
        FuzzyRegexBuilder::new("test").dot_all(true),
        FuzzyRegexBuilder::new("test").similarity(0.5),
    ];

    for builder in options {
        let _ = builder.build();
    }
}

/// Test partial matching API
#[test]
fn test_partial_matching() {
    use fuzzy_regex::FuzzyRegexBuilder;

    // Without partial (default) - partial flag is always false
    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(false) // default
        .build()
        .unwrap();

    let m = re.find("hello").unwrap();
    assert!(!m.partial());

    // With partial enabled - match at end of text is marked as partial
    let re2 = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(true)
        .build()
        .unwrap();

    // Match reaches end of text -> partial
    let m2 = re2.find("hello").unwrap();
    assert!(m2.partial());

    // Match doesn't reach end of text -> not partial
    // "say hello world" has match at positions 4-9, but text is 15 chars
    let m3 = re2.find("say hello world").unwrap();
    assert!(!m3.partial());

    // Fuzzy match reaching end is also partial
    let m4 = re2.find("hallo").unwrap(); // "hallo" matches with 1 substitution
    assert!(m4.partial());
}

/// Fuzz test: edge cases
#[test]
fn fuzz_edge_cases() {
    // Empty patterns
    let _ = FuzzyRegex::new("");

    // Single character patterns
    let _ = FuzzyRegex::new("a");
    let _ = FuzzyRegex::new("(?:a){e<=1}");

    // Very long patterns
    let long_pattern = "a".repeat(1000);
    let _ = FuzzyRegex::new(&long_pattern);

    // Very long text
    let long_text = "hello world ".repeat(100);
    if let Ok(re) = FuzzyRegex::new("hello") {
        let _ = re.is_match(&long_text);
        let _ = re.find(&long_text);
    }

    // Unicode text
    let unicode_texts = vec!["привет мир", "こんにちは", "🎉🎊🎁"];
    if let Ok(re) = FuzzyRegex::new("(?:привет){e<=1}") {
        for text in unicode_texts {
            let _ = re.is_match(text);
        }
    }
}

/// Fuzz test: replace
#[test]
fn fuzz_replace() {
    let patterns = ["hello", "(?:world){e<=1}", "(\\w+)"];

    for pattern in patterns {
        if let Ok(re) = FuzzyRegex::new(pattern) {
            let _ = re.replace("hello world", "REPLACED");
            let _ = re.replace_all("hello world hello", "X");
        }
    }
}

/// Fuzz test: split
#[test]
fn fuzz_split() {
    let patterns = [",", "\\s+"];

    for pattern in patterns {
        if let Ok(re) = FuzzyRegex::new(pattern) {
            let _ = re.split("a,b,c").collect::<Vec<_>>();
            let _ = re.splitn("a,b,c,d", 2);
        }
    }
}

/// Fuzz test: similarity threshold
#[test]
fn fuzz_similarity() {
    for similarity in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let builder = FuzzyRegexBuilder::new("test").similarity(similarity);
        let _ = builder.build();
    }
}

/// Fuzz test: word lists
#[test]
fn fuzz_word_lists() {
    let mut re = FuzzyRegex::new(r"\L<words>").unwrap();
    re.set_word_list("words", vec!["cat", "dog", "bird"]);

    let texts = vec!["cat", "dog", "cat and dog", "bird", "", "elephant"];

    for text in texts {
        let _ = re.is_match(text);
        let _ = re.find(text);
        let _ = re.find_iter(text).collect::<Vec<_>>();
    }
}

/// An unresolved `\L<name>` (no `set_word_list`) is an empty alternation and
/// must match nothing across every entry point — previously it silently matched
/// the empty string everywhere (`(0,0)`, `is_match == true`).
#[test]
fn unresolved_named_list_matches_nothing() {
    for pat in [r"\b\L<words>\b", r"\b\L<words>{e<=1}\b", r"\L<words>"] {
        let re = FuzzyRegex::new(pat).unwrap();
        let text = "a dog x";
        assert!(re.find(text).is_none(), "find [{pat}]");
        assert!(!re.is_match(text), "is_match [{pat}]");
        assert_eq!(re.find_iter(text).count(), 0, "find_iter [{pat}]");
        assert!(re.captures(text).is_none(), "captures [{pat}]");
        assert!(re.find_at(text, 0).is_none(), "find_at [{pat}]");
        // Empty input must not yield a spurious empty match either.
        assert!(re.find("").is_none(), "find empty [{pat}]");
        assert!(!re.is_match(""), "is_match empty [{pat}]");
    }
}

/// Once the list is provided, `\L<name>` matches the words (and only the words).
#[test]
fn resolved_named_list_matches_words() {
    let mut re = FuzzyRegex::new(r"\b\L<words>\b").unwrap();
    re.set_word_list("words", vec!["cat", "dog", "frog"]);
    assert_eq!(
        re.find("a dog x").map(|m| (m.start(), m.end())),
        Some((2, 5))
    );
    assert!(re.is_match("cat"));
    assert!(re.find("a cow x").is_none());

    // A reference to a *different*, still-unset list stays unmatchable even when
    // some other list is populated.
    let mut re2 = FuzzyRegex::new(r"\L<a>\L<b>").unwrap();
    re2.set_word_list("a", vec!["x"]);
    assert!(!re2.is_match("xy"), "list `b` still unresolved -> no match");
}

/// The capture / find_at entry points must be word-list-aware too: with a
/// resolved list, `captures`, `captures_at`, `find_at`, and
/// `captures_all_overlapping` return the matched word (group 0), consistent with
/// `find` — previously they used the NFA (which doesn't expand named lists) and
/// returned no match.
#[test]
fn resolved_named_list_captures_match_find() {
    let mut re = FuzzyRegex::new(r"\b\L<words>\b").unwrap();
    re.set_word_list("words", vec!["cat", "dog", "frog"]);

    let text = "a dog x";
    let find = re.find(text).map(|m| (m.start(), m.end()));
    assert_eq!(find, Some((2, 5)));

    // captures group 0 must equal find()
    let caps = re.captures(text).expect("captures should match");
    let g0 = caps.get(0).expect("group 0");
    assert_eq!((g0.start(), g0.end()), (2, 5));
    assert_eq!(g0.as_str(), "dog");

    // captures_at from before the match finds it; from after does not
    assert!(re.captures_at(text, 0).is_some());
    assert!(re.captures_at(text, 5).is_none(), "nothing after index 5");

    // find_at anchors the match at `start` (it does not scan forward — this is
    // find_at's contract for every pattern, and \L is now consistent with it):
    // nothing matches AT position 0, but the word matches AT position 2.
    assert!(re.find_at(text, 0).is_none());
    assert_eq!(
        re.find_at(text, 2).map(|m| (m.start(), m.end())),
        Some((2, 5))
    );

    // captures_all_overlapping finds every word occurrence
    let all = re.captures_all_overlapping("dog cat", 0.0);
    assert!(!all.is_empty(), "should find dog and cat");

    // No match still yields no captures.
    assert!(re.captures("a cow x").is_none());
}

/// `\L<name>` is expanded into a real NFA alternation on `set_word_list`, so it
/// composes correctly with the rest of the pattern: surrounding anchors are
/// honored, it can appear inside a larger pattern, sub-captures work, and a
/// clone preserves the resolved list.
#[test]
fn named_list_expands_into_nfa_alternation() {
    // Surrounding \b anchors are honored (the old bespoke matcher ignored them).
    let mut re = FuzzyRegex::new(r"\b\L<w>\b").unwrap();
    re.set_word_list("w", vec!["dog", "cat"]);
    assert_eq!(
        re.find("a dog x").map(|m| (m.start(), m.end())),
        Some((2, 5))
    );
    assert!(re.find("adogx").is_none(), "\\b must be honored");

    // \L embedded in a larger pattern, with a capture group around other parts.
    let mut kv = FuzzyRegex::new(r"(\w+)=\L<v>").unwrap();
    kv.set_word_list("v", vec!["on", "off"]);
    let caps = kv.captures("mode=off").expect("should match");
    assert_eq!(
        caps.get(0).map(|m| m.as_str().to_string()),
        Some("mode=off".into())
    );
    assert_eq!(
        caps.get(1).map(|m| m.as_str().to_string()),
        Some("mode".into())
    );
    assert!(kv.find("mode=blue").is_none(), "blue not in list");

    // Fuzzy over the alternation.
    let mut fz = FuzzyRegex::new(r"\b\L<w>{e<=1}\b").unwrap();
    fz.set_word_list("w", vec!["dog", "cat"]);
    assert!(fz.is_match("cot")); // 1 sub from cat

    // Clone preserves the resolved list (previously a clone re-compiled from the
    // pattern and dropped the word list).
    let cloned = re.clone();
    assert_eq!(
        cloned.find("a cat y").map(|m| (m.start(), m.end())),
        Some((2, 5))
    );

    // An empty list is treated as unresolved (matches nothing).
    let mut empty = FuzzyRegex::new(r"\L<w>").unwrap();
    empty.set_word_list("w", Vec::<&str>::new());
    assert!(!empty.is_match("dog"));
}
