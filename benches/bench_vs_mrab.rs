//! Benchmark to compare with Python mrab-regex performance.

use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn bench<F: Fn()>(name: &str, iterations: usize, func: F) -> f64 {
    // Warmup
    for _ in 0..5 {
        func();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        func();
    }
    let elapsed = start.elapsed();
    let per_iter_us = elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64;
    println!("{:50} {:>12.2} us/iter", name, per_iter_us);
    per_iter_us
}

fn main() {
    println!("Rust fuzzy-regex Benchmark");
    println!("==========================\n");

    // Test 1: Short text, simple fuzzy
    let short_text = "The quick brown fox jumps over the lazy dog.";
    println!("Test 1: Short text ({} bytes)", short_text.len());

    let re1 = FuzzyRegex::new(r"(?:quick){e<=1}").unwrap();
    bench("  find 'quick' with e<=1", 10000, || {
        std::hint::black_box(re1.find(std::hint::black_box(short_text)));
    });

    // Test 2: Medium text
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                       Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
                       Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    println!("\nTest 2: Medium text ({} bytes)", medium_text.len());

    let re2 = FuzzyRegex::new(r"(?:Lorem){e<=2}").unwrap();
    bench("  find 'Lorem' with e<=2", 1000, || {
        std::hint::black_box(re2.find(std::hint::black_box(medium_text)));
    });

    // Test 3: Long text (4KB)
    let long_text = medium_text.repeat(20);
    println!("\nTest 3: Long text ({} bytes)", long_text.len());

    let re3 = FuzzyRegex::new(r"(?:Lorem){e<=2}").unwrap();
    bench("  find 'Lorem' with e<=2", 100, || {
        std::hint::black_box(re3.find(std::hint::black_box(&long_text)));
    });

    // Test 4: Pattern matching with substitution constraint
    println!("\nTest 4: Substitution constraint");
    let re4 = FuzzyRegex::new(r"(?:quick){s<=1}").unwrap();
    bench("  find 'quick' with s<=1 (short)", 10000, || {
        std::hint::black_box(re4.find(std::hint::black_box(short_text)));
    });

    // Test 5: No match (worst case - full scan)
    println!("\nTest 5: No match (full scan)");
    let re5 = FuzzyRegex::new(r"(?:xyzzy){e<=1}").unwrap();
    bench("  find 'xyzzy' e<=1 (short, no match)", 10000, || {
        std::hint::black_box(re5.find(std::hint::black_box(short_text)));
    });
    bench("  find 'xyzzy' e<=1 (medium, no match)", 1000, || {
        std::hint::black_box(re5.find(std::hint::black_box(medium_text)));
    });

    // Test 6: DNA sequence
    println!("\nTest 6: DNA sequence (1000 bp)");
    let dna: String = (0..1000).map(|i| ["A", "C", "G", "T"][i % 4]).collect();
    let re6 = FuzzyRegex::new(r"(?:ACGTACGT){e<=2}").unwrap();
    bench("  find motif with e<=2", 100, || {
        std::hint::black_box(re6.find(std::hint::black_box(&dna)));
    });

    println!("\nDone!");
}
