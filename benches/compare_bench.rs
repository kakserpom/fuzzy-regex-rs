//! Benchmark for comparing with mrab-regex (Python/C).
//! Run with: cargo bench --bench compare_bench

use fuzzy_regex::{FuzzyRegex, FuzzyRegexBuilder};
use std::time::Instant;

fn bench<F>(name: &str, iterations: u32, mut f: F) -> f64
where
    F: FnMut(),
{
    // Warmup
    for _ in 0..5 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();

    let per_iter_us = elapsed.as_nanos() as f64 / 1000.0 / iterations as f64;
    println!("{:50} {:>12.2} us/iter", name, per_iter_us);
    per_iter_us
}

fn main() {
    println!("Rust fuzzy-regex Benchmark");
    println!("==========================\n");

    // Test 1: Short text, simple fuzzy
    let short_text = "The quick brown fox jumps over the lazy dog.";
    println!("Test 1: Short text ({} bytes)", short_text.len());

    let re1 = FuzzyRegex::new("(?:quick){e<=1}").unwrap();
    bench("  find 'quick' with e<=1", 10000, || {
        let _ = re1.find(short_text);
    });

    // Test 2: Medium text
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    println!("\nTest 2: Medium text ({} bytes)", medium_text.len());

    let re2 = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();
    bench("  find 'Lorem' with e<=2", 1000, || {
        let _ = re2.find(medium_text);
    });

    // Test 3: Long text (4KB)
    let long_text = medium_text.repeat(20);
    println!("\nTest 3: Long text ({} bytes)", long_text.len());

    let re3 = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();
    bench("  find 'Lorem' with e<=2", 100, || {
        let _ = re3.find(&long_text);
    });

    // Test 4: Pattern matching with substitution constraint
    println!("\nTest 4: Substitution constraint");
    let re4 = FuzzyRegex::new("(?:quick){s<=1}").unwrap();
    bench("  find 'quick' with s<=1 (short)", 10000, || {
        let _ = re4.find(short_text);
    });

    // Test 5: No match (worst case - full scan)
    println!("\nTest 5: No match (full scan)");
    let re5 = FuzzyRegex::new("(?:xyzzy){e<=1}").unwrap();
    bench("  find 'xyzzy' e<=1 (short, no match)", 10000, || {
        let _ = re5.find(short_text);
    });
    bench("  find 'xyzzy' e<=1 (medium, no match)", 1000, || {
        let _ = re5.find(medium_text);
    });

    // Test 6: DNA sequence
    println!("\nTest 6: DNA sequence (1000 bp)");
    let dna: String = (0..1000)
        .map(|i| match i % 4 {
            0 => 'A',
            1 => 'C',
            2 => 'G',
            _ => 'T',
        })
        .collect();
    let re6 = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();
    bench("  find motif with e<=2", 100, || {
        let _ = re6.find(&dna);
    });

    // Test 7: Greedy first mode (mrab-regex like)
    println!("\n=== GREEDY FIRST MODE (mrab-regex like) ===\n");

    println!("Test 7: Short text with greedy_first");
    let re7 = FuzzyRegexBuilder::new("(?:quick){e<=1}")
        .greedy_first(true)
        .build()
        .unwrap();
    bench("  find 'quick' with e<=1 (greedy)", 10000, || {
        let _ = re7.find(short_text);
    });

    println!("\nTest 8: Medium text with greedy_first");
    let re8 = FuzzyRegexBuilder::new("(?:Lorem){e<=2}")
        .greedy_first(true)
        .build()
        .unwrap();
    bench("  find 'Lorem' with e<=2 (greedy)", 1000, || {
        let _ = re8.find(medium_text);
    });

    println!("\nTest 9: Long text with greedy_first");
    let re9 = FuzzyRegexBuilder::new("(?:Lorem){e<=2}")
        .greedy_first(true)
        .build()
        .unwrap();
    bench("  find 'Lorem' with e<=2 (greedy)", 100, || {
        let _ = re9.find(&long_text);
    });

    println!("\nTest 10: DNA with greedy_first");
    let re10 = FuzzyRegexBuilder::new("(?:ACGTACGT){e<=2}")
        .greedy_first(true)
        .build()
        .unwrap();
    bench("  find motif with e<=2 (greedy)", 100, || {
        let _ = re10.find(&dna);
    });

    println!("\nDone!");
}
