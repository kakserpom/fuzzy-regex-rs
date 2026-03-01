//! Micro-benchmark for SIMD vs scalar Bitap
//!
//! Run with: cargo run --release --example `simd_bench`

use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iterations: u32, mut f: F) -> f64 {
    // Warmup
    for _ in 0..100 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();

    let per_iter_ns = elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);
    println!("{name:40} {per_iter_ns:>8.1} ns/iter");
    per_iter_ns
}

fn main() {
    println!("SIMD vs Scalar Bitap Benchmark\n");

    // DNA sequence - uses streaming Bitap path
    let dna: String = (0..1000).map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' }).collect();

    // Pattern at position 0 - should be found immediately
    println!("Test 1: Pattern at start (should use SIMD)");
    let re1 = FuzzyRegexBuilder::new("(?:ACGT){e<=2}")
        .build()
        .unwrap();
    bench("  ACGT e<=2 in DNA (1000bp)", 100_000, || {
        let _ = re1.find(&dna);
    });

    // Pattern not at start - tests streaming performance
    println!("\nTest 2: Streaming through DNA");
    let dna_nomatch: String = (0..1000).map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' }).collect();
    let re2 = FuzzyRegexBuilder::new("(?:XXXX){e<=2}")
        .build()
        .unwrap();
    bench("  XXXX e<=2 in DNA (no match)", 10_000, || {
        let _ = re2.find(&dna_nomatch);
    });

    // ASCII text pattern
    println!("\nTest 3: English text");
    let text = "The quick brown fox jumps over the lazy dog. ".repeat(100);
    let re3 = FuzzyRegexBuilder::new("(?:quick){e<=2}")
        .build()
        .unwrap();
    bench("  'quick' e<=2 in text (4400b)", 10_000, || {
        let _ = re3.find(&text);
    });

    // Compare k=1 vs k=2 vs k=3
    println!("\nTest 4: Different edit distances (DNA 1000bp)");
    for k in 1..=3 {
        let pattern = format!("(?:ACGTACGT){{e<={k}}}");
        let re = FuzzyRegexBuilder::new(&pattern)
            .build()
            .unwrap();
        bench(&format!("  ACGTACGT e<={k}"), 100_000, || {
            let _ = re.find(&dna);
        });
    }

    println!("\nDone!");
}
