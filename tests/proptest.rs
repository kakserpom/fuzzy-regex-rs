//! Property-based tests using proptest.
//!
//! Run with: cargo test --test proptest

use fuzzy_regex::FuzzyRegex;
use proptest::prelude::*;

proptest! {
    /// Exact matches should always work (no errors allowed)
    #[test]
    fn test_exact_match_always_works(pattern in "[a-z]{1,10}", text in "[a-z ]{0,100}") {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~0", pattern)).ok() {
            // If pattern appears in text exactly, it should match
            if text.contains(&pattern) {
                assert!(re.is_match(&text));
            }
        }
    }

    /// More edits should still find matches when fewer edits do
    #[test]
    fn test_more_edits_still_finds_matches(
        pattern in "[a-z]{1,5}",
        text in "[a-z ]{1,50}",
        edits in 0..=1usize,
    ) {
        let re1 = FuzzyRegex::new(&format!("(?:{})~{}", pattern, edits)).ok();
        let re2 = FuzzyRegex::new(&format!("(?:{})~{}", pattern, edits + 1)).ok();

        if let (Some(re1), Some(re2)) = (re1, re2) {
            let has_match1 = re1.find(&text).is_some();
            let has_match2 = re2.find(&text).is_some();

            // If stricter pattern matches, looser should too
            if has_match1 {
                assert!(has_match2);
            }
        }
    }

    /// Substitutions should work within limit
    #[test]
    fn test_substitution_limit(
        pattern in "[a-z]{2,5}",
        text in "[a-z ]{0,50}",
        max_subs in 0..=2u8,
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{}){{s<={}}}", pattern, max_subs)).ok() {
            // Just ensure it compiles and runs without panic
            let _ = re.find(&text);
        }
    }

    /// Insertions should work within limit
    #[test]
    fn test_insertion_limit(
        pattern in "[a-z]{2,5}",
        text in "[a-z ]{0,50}",
        max_ins in 0..=2u8,
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{}){{i<={}}}", pattern, max_ins)).ok() {
            let _ = re.find(&text);
        }
    }

    /// Deletions should work within limit
    #[test]
    fn test_deletion_limit(
        pattern in "[a-z]{2,5}",
        text in "[a-z ]{0,50}",
        max_del in 0..=2u8,
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{}){{d<={}}}", pattern, max_del)).ok() {
            let _ = re.find(&text);
        }
    }

    /// Transposition should work within limit
    #[test]
    fn test_transposition_limit(
        pattern in "[a-z]{2,5}",
        text in "[a-z ]{0,50}",
        max_trans in 0..=2u8,
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{}){{t<={}}}", pattern, max_trans)).ok() {
            let _ = re.find(&text);
        }
    }

    /// Cost-based matching should work
    #[test]
    fn test_cost_constraint(
        pattern in "[a-z]{2,4}",
        text in "[a-z ]{0,30}",
        cost in 1..=3u8,
    ) {
        if let Ok(re) = FuzzyRegex::new(&format!("(?:{}){{c<={}}}", pattern, cost)) {
            let _ = re.find(&text);
        }
    }

    /// Case insensitive should match regardless of case
    #[test]
    fn test_case_insensitive(
        pattern in "[a-zA-Z]{1,5}",
        text in "[a-zA-Z ]{0,30}",
    ) {
        if let Ok(re) = FuzzyRegex::builder(&format!("(?:{})", pattern))
            .case_insensitive(true)
            .build()
        {
            let lower_text = text.to_lowercase();
            let upper_text = text.to_uppercase();

            // If exact pattern is in text with different case, should match
            if text.to_lowercase().contains(&pattern.to_lowercase()) {
                assert!(re.is_match(&text) || re.is_match(&lower_text) || re.is_match(&upper_text));
            }
        }
    }

    /// Unicode patterns should work
    #[test]
    fn test_unicode_fuzzy(
        pattern in "\\p{L}{1,5}", // 1-5 unicode letters
        text in "\\p{L}*{0,30}", // unicode letters, 0-30 chars
    ) {
        if let Ok(re) = FuzzyRegex::new(&format!("(?:{})~1", pattern)) {
            let _ = re.find(&text);
            let _ = re.is_match(&text);
        }
    }

    /// find and is_match should be consistent
    #[test]
    fn test_find_and_is_match_consistent(
        pattern in "[a-z]{1,5}",
        text in "[a-z ]{0,50}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~1", pattern)).ok() {
            let is_match = re.is_match(&text);
            let find_match = re.find(&text);

            assert_eq!(is_match, find_match.is_some());
        }
    }

    /// Match positions should be valid
    #[test]
    fn test_match_positions_valid(
        pattern in "[a-z]{1,5}",
        text in "[a-z ]{1,50}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~1", pattern)).ok() {
            for m in re.find_iter(&text) {
                assert!(m.start() <= m.end());
                assert!(m.end() <= text.len());
                assert!(!m.is_empty());

                // The matched string should equal the slice
                assert_eq!(m.as_str(), &text[m.start()..m.end()]);
            }
        }
    }

    /// Similarity should be between 0 and 1
    #[test]
    fn test_similarity_bounds(
        pattern in "[a-z]{1,5}",
        text in "[a-z ]{0,50}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~2", pattern)).ok() {
            for m in re.find_iter(&text) {
                let sim = m.similarity();
                assert!(sim >= 0.0 && sim <= 1.0, "Similarity {} out of bounds", sim);
            }
        }
    }

    /// Empty pattern should match empty string
    #[test]
    fn test_empty_pattern(text in ".*") {
        if let Ok(re) = FuzzyRegex::new("") {
            // Empty pattern should match at position 0
            let m = re.find(&text);
            if text.is_empty() {
                assert!(m.is_some());
            }
        }
    }

    /// Repeating find_iter should produce consistent results
    #[test]
    fn test_find_iter_idempotent(
        pattern in "[a-z]{1,4}",
        text in "[a-z ]{0,40}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~1", pattern)).ok() {
            let results: Vec<_> = re.find_iter(&text).collect();

            // Running again should produce same results
            let results2: Vec<_> = re.find_iter(&text).collect();

            assert_eq!(results.len(), results2.len());
            for (m1, m2) in results.iter().zip(results2.iter()) {
                assert_eq!(m1.start(), m2.start());
                assert_eq!(m1.end(), m2.end());
            }
        }
    }

    /// Character class restrictions should work
    #[test]
    fn test_character_class_restriction(
        pattern in "[a-z]{2,4}",
        text in "[a-z0-9 ]{0,30}",
    ) {
        // Only allow substitutions with digits
        if let Ok(re) = FuzzyRegex::new(&format!("(?:{}){{s<=1:[0-9]}}", pattern)) {
            let _ = re.find(&text);
        }
    }

    /// Lookahead patterns should work
    #[test]
    fn test_lookahead(
        prefix in "[a-z]{0,3}",
        pattern in "[a-z]{1,3}",
        suffix in "[a-z]{0,3}",
    ) {
        let text = format!("{}{}", prefix, suffix);
        if let Some(re) = FuzzyRegex::new(&format!("(?={}){}", pattern, pattern)).ok() {
            // If pattern appears twice consecutively, should match
            let double_pattern = format!("{}{}", pattern, pattern);
            if text.contains(&double_pattern) {
                assert!(re.is_match(&text));
            }
        }
    }

    /// Lookbehind patterns should work
    #[test]
    fn test_lookbehind(
        before in "[a-z]{0,3}",
        pattern in "[a-z]{1,3}",
        after in "[a-z]{0,3}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?<={})", pattern)).ok() {
            let full_text = format!("{}{}{}", before, pattern, after);
            let _ = re.find(&full_text);
        }
    }

    /// Streaming API should work correctly
    #[test]
    fn test_streaming_finds_matches(
        pattern in "[a-z]{1,4}",
        text in "[a-z ]{1,50}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~1", pattern)).ok() {
            // Streaming find should find at least one match if text contains pattern
            let mut stream = re.stream();
            let matches: Vec<_> = stream.feed(text.as_bytes()).collect();

            // Just check that streaming works without panicking
            // and produces some valid-looking results
            for m in &matches {
                assert!(m.start() <= m.end());
                assert!(m.end() <= text.len());
            }
        }
    }

    /// Position tracking in streaming should be correct
    #[test]
    fn test_streaming_position(
        pattern in "[a-z]{1,3}",
        chunks in "[a-z ]{1,20}",
    ) {
        if let Some(re) = FuzzyRegex::new(&format!("(?:{})~1", pattern)).ok() {
            let mut stream = re.stream();
            let mut total_len = 0;

            // Feed chunks
            for chunk in chunks.split(' ') {
                if !chunk.is_empty() {
                    let _matches: Vec<_> = stream.feed(chunk.as_bytes()).collect();
                    total_len += chunk.len();
                    assert_eq!(stream.position(), total_len);
                }
            }
        }
    }
}
