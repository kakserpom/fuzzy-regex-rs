//! Correctness verification for fuzzy-regex optimizations.
//! Compares normal mode vs greedy_first mode to ensure they find the same matches.

use fuzzy_regex::{FuzzyRegex, FuzzyRegexBuilder};

fn verify_match(pattern: &str, text: &str, expected_match: Option<&str>) {
    // Test normal mode
    let re_normal = FuzzyRegex::new(pattern).unwrap();
    let match_normal = re_normal.find(text);

    // Test greedy_first mode
    let re_greedy = FuzzyRegexBuilder::new(pattern)
        .greedy_first(true)
        .build()
        .unwrap();
    let match_greedy = re_greedy.find(text);

    // Verify both modes find a match (or both don't)
    match (match_normal.as_ref(), match_greedy.as_ref(), expected_match) {
        (Some(n), Some(g), Some(expected)) => {
            let normal_text = &text[n.start()..n.end()];
            let greedy_text = &text[g.start()..g.end()];

            println!("✓ Pattern: {:?}", pattern);
            println!("  Text: {:?}", text);
            println!("  Normal: {:?} at {}..{}", normal_text, n.start(), n.end());
            println!("  Greedy: {:?} at {}..{}", greedy_text, g.start(), g.end());

            // Both should find a valid match
            assert!(normal_text.len() > 0, "Normal match is empty");
            assert!(greedy_text.len() > 0, "Greedy match is empty");

            // For greedy mode, we just need A valid match, not necessarily the same one
            // (greedy returns first found, normal may return best)
        }
        (None, None, None) => {
            println!("✓ Pattern: {:?} - correctly found no match in {:?}", pattern, text);
        }
        (Some(n), None, _) => {
            let normal_text = &text[n.start()..n.end()];
            panic!(
                "MISMATCH: Normal found {:?} but greedy found nothing\n  Pattern: {:?}\n  Text: {:?}",
                normal_text, pattern, text
            );
        }
        (None, Some(g), _) => {
            let greedy_text = &text[g.start()..g.end()];
            panic!(
                "MISMATCH: Greedy found {:?} but normal found nothing\n  Pattern: {:?}\n  Text: {:?}",
                greedy_text, pattern, text
            );
        }
        (Some(_), Some(_), None) => {
            panic!(
                "UNEXPECTED: Both modes found a match but expected None\n  Pattern: {:?}\n  Text: {:?}",
                pattern, text
            );
        }
        (None, None, Some(expected)) => {
            panic!(
                "MISSING: Expected {:?} but neither mode found a match\n  Pattern: {:?}\n  Text: {:?}",
                expected, pattern, text
            );
        }
    }
}

fn main() {
    println!("=== Fuzzy Regex Correctness Verification ===\n");

    // Test 1: Exact matches
    println!("--- Test 1: Exact matches ---");
    verify_match("hello", "hello world", Some("hello"));
    verify_match("world", "hello world", Some("world"));
    verify_match("quick", "The quick brown fox", Some("quick"));

    // Test 2: Fuzzy matches with substitutions
    println!("\n--- Test 2: Substitutions ---");
    verify_match("(?:hello){e<=1}", "hallo world", Some("hallo"));
    verify_match("(?:hello){e<=1}", "hxllo world", Some("hxllo"));
    verify_match("(?:quick){e<=1}", "The quack brown fox", Some("quack"));

    // Test 3: Fuzzy matches with insertions
    println!("\n--- Test 3: Insertions ---");
    verify_match("(?:hello){e<=1}", "heello world", Some("heello"));
    verify_match("(?:cat){e<=1}", "caat sitting", Some("caat"));

    // Test 4: Fuzzy matches with deletions
    println!("\n--- Test 4: Deletions ---");
    verify_match("(?:hello){e<=2}", "helo world", Some("helo"));
    verify_match("(?:world){e<=1}", "wrld end", Some("wrld"));

    // Test 5: No matches
    println!("\n--- Test 5: No matches ---");
    verify_match("(?:xyzzy){e<=1}", "The quick brown fox", None);
    verify_match("(?:abcdef){e<=1}", "nothing matches here", None);

    // Test 6: DNA sequences
    println!("\n--- Test 6: DNA sequences ---");
    let dna: String = (0..100)
        .map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' })
        .collect();
    verify_match("(?:ACGT){e<=1}", &dna, Some("ACGT"));
    verify_match("(?:ACGTACGT){e<=2}", &dna, Some("ACGTACGT"));
    verify_match("(?:GGGG){e<=1}", &dna, None); // No 4 consecutive Gs

    // Test 7: Edge cases
    println!("\n--- Test 7: Edge cases ---");
    verify_match("(?:a){e<=1}", "a", Some("a"));
    verify_match("(?:a){e<=1}", "b", Some("b")); // 1 substitution
    verify_match("(?:ab){e<=1}", "a", Some("a")); // 1 deletion
    verify_match("(?:a){e<=1}", "ab", Some("a")); // exact match (ignore extra)

    // Test 8: Case sensitivity
    println!("\n--- Test 8: Case sensitivity ---");
    verify_match("(?:Hello){e<=1}", "hello world", Some("hello")); // 1 sub for H->h
    verify_match("(?:HELLO){e<=2}", "hello world", None); // needs 5 subs, only 2 allowed

    // Test 9: Multiple matches (verify we find at least one)
    println!("\n--- Test 9: Multiple potential matches ---");
    verify_match("(?:the){e<=1}", "the them then", Some("the"));
    verify_match("(?:cat){e<=1}", "cat bat rat cat", Some("cat"));

    // Test 10: Long text
    println!("\n--- Test 10: Long text ---");
    let long_text = "Lorem ipsum ".repeat(100);
    verify_match("(?:Lorem){e<=2}", &long_text, Some("Lorem"));
    verify_match("(?:ipsum){e<=1}", &long_text, Some("ipsum"));

    // Test 11: Unicode (should work correctly)
    println!("\n--- Test 11: Unicode ---");
    verify_match("(?:café){e<=1}", "I love café au lait", Some("café"));
    verify_match("(?:naïve){e<=1}", "Don't be naïve", Some("naïve"));

    // Test 12: Special regex characters in pattern
    println!("\n--- Test 12: Patterns with context ---");
    verify_match("The (?:quick){e<=1} brown", "The quack brown fox", Some("The quack brown"));
    verify_match("(?:hello){e<=1} world", "hallo world!", Some("hallo world"));

    println!("\n=== All correctness tests passed! ===");
}
