#![allow(clippy::cast_precision_loss, clippy::unreadable_literal)]
//! Direct comparison benchmark with mrab-regex test cases

use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) -> f64 {
    // Warmup
    for _ in 0..10 { f(); }

    let start = Instant::now();
    for _ in 0..iters { f(); }
    let us = start.elapsed().as_nanos() as f64 / f64::from(iters) / 1000.0;
    println!("{name:55} {us:>10.2} µs/iter");
    us
}

fn main() {
    println!("=== Direct Comparison with mrab-regex ===\n");
    println!("Run bench_python.py alongside to compare\n");

    // Test 1: Short text, simple fuzzy
    let short_text = "The quick brown fox jumps over the lazy dog.";
    println!("Test 1: Short text ({} bytes)", short_text.len());

    let re1 = FuzzyRegex::new("(?:quick){e<=1}").unwrap();
    bench("  find 'quick' with e<=1", 10000, || { let _ = re1.find(short_text); });

    // Test 2: Medium text
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    println!("\nTest 2: Medium text ({} bytes)", medium_text.len());

    let re2 = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();
    bench("  find 'Lorem' with e<=2", 1000, || { let _ = re2.find(medium_text); });

    // Test 3: Long text (4KB)
    let long_text = medium_text.repeat(20);
    println!("\nTest 3: Long text ({} bytes)", long_text.len());

    let re3 = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();
    bench("  find 'Lorem' with e<=2", 100, || { let _ = re3.find(&long_text); });

    // Test 4: Pattern matching with substitution constraint
    println!("\nTest 4: Substitution constraint");
    let re4 = FuzzyRegex::new("(?:quick){s<=1}").unwrap();
    bench("  find 'quick' with s<=1 (short)", 10000, || { let _ = re4.find(short_text); });

    // Test 5: No match (worst case - full scan)
    println!("\nTest 5: No match (full scan)");
    let re5 = FuzzyRegex::new("(?:xyzzy){e<=1}").unwrap();
    bench("  find 'xyzzy' e<=1 (short, no match)", 10000, || { let _ = re5.find(short_text); });
    bench("  find 'xyzzy' e<=1 (medium, no match)", 1000, || { let _ = re5.find(medium_text); });

    // Test 6: DNA sequence
    println!("\nTest 6: DNA sequence (1000 bp)");
    let dna: String = (0..1000).map(|i| ["A", "C", "G", "T"][i % 4]).collect();
    let re6 = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();
    bench("  find motif with e<=2", 100, || { let _ = re6.find(&dna); });

    // Additional tests
    println!("\n--- Additional Tests ---");

    // Test 7: Very long text with no match
    let very_long = medium_text.repeat(100);
    println!("\nTest 7: Very long text no-match ({} bytes)", very_long.len());
    bench("  find 'xyzzy' e<=1 (very long, no match)", 100, || { let _ = re5.find(&very_long); });

    // Test 8: Match at end
    println!("\nTest 8: Match at end position");
    let text_end = format!("{}quick", "x".repeat(1000));
    let re8 = FuzzyRegex::new("(?:quick){e<=1}").unwrap();
    bench("  find 'quick' e<=1 at position 1000", 1000, || { let _ = re8.find(&text_end); });

    // Test 9: Russian text
    println!("\nTest 9: Russian/Cyrillic text");
    let russian = "Привет мир! Это тестовый текст на русском языке.";
    let re9 = FuzzyRegex::new("(?:Привет){e<=1}").unwrap();
    bench("  find 'Привет' e<=1", 10000, || { let _ = re9.find(russian); });

    // Test 10: Alternation
    println!("\nTest 10: Alternation patterns");
    let re10a = FuzzyRegex::new("(?:quick|brown){e<=1}").unwrap();
    let re10b = FuzzyRegex::new("(?:quick|brown|fox|lazy|dog){e<=1}").unwrap();
    bench("  2-alt pattern", 10000, || { let _ = re10a.find(short_text); });
    bench("  5-alt pattern", 10000, || { let _ = re10b.find(short_text); });

    println!("\nDone!");
}
