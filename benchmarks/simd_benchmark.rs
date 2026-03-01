//! Benchmark specifically comparing SIMD vs scalar paths.

use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

fn main() {
    println!("=== SIMD vs Scalar Benchmark ===\n");

    #[cfg(target_arch = "aarch64")]
    println!("Architecture: ARM64 (NEON available)\n");

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            println!("Architecture: x86_64 (AVX2 available)\n");
        } else {
            println!("Architecture: x86_64 (no AVX2)\n");
        }
    }

    let iterations: u32 = 100_000;

    // Test 1: Short ASCII pattern with k=1 (SIMD should help)
    println!("--- Test 1: ASCII k=1 (SIMD path) ---");
    test_pattern("fox", 1, "The quick brown fox jumps over the lazy dog", iterations);

    // Test 2: Short ASCII pattern with k=0 (exact match)
    println!("\n--- Test 2: ASCII k=0 (SIMD path) ---");
    test_pattern("fox", 0, "The quick brown fox jumps over the lazy dog", iterations);

    // Test 3: ASCII pattern with k=2 (scalar path on ARM, SIMD on x86)
    println!("\n--- Test 3: ASCII k=2 ---");
    test_pattern("quick", 2, "The quick brown fox jumps over the lazy dog", iterations);

    // Test 4: Longer text with k=1
    println!("\n--- Test 4: Longer text, k=1 ---");
    let long_text = "The quick brown fox jumps over the lazy dog. ".repeat(100);
    test_pattern("jumps", 1, &long_text, iterations / 10);

    // Test 5: Pattern at end of text (worst case - full scan)
    println!("\n--- Test 5: Pattern at end ---");
    test_pattern("dog", 1, "The quick brown fox jumps over the lazy dog", iterations);

    // Test 6: Multiple short patterns
    println!("\n--- Test 6: Multiple short patterns ---");
    let regex = FuzzyRegexBuilder::new("(?:cat|dog|fox)")
        .edits(1)
        .similarity(0.6)
        .build()
        .unwrap();

    let text = "The quick brown fox jumps over the lazy dog";
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(regex.find(text));
    }
    let elapsed = start.elapsed();
    let per_iter_ns = elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);
    println!("Time per find: {per_iter_ns:.0} ns");
    let bytes = u32::try_from(text.len()).expect("text too large");
    let throughput = f64::from(bytes) * f64::from(iterations) / elapsed.as_secs_f64() / 1_000_000.0;
    println!("Throughput: {throughput:.1} MB/s");
}

fn test_pattern(pattern: &str, edits: u8, text: &str, iterations: u32) {
    let regex = FuzzyRegexBuilder::new(&format!("(?:{pattern})"))
        .edits(edits)
        .similarity(0.5)
        .build()
        .unwrap();

    // Warmup
    for _ in 0..1000 {
        std::hint::black_box(regex.find(text));
    }

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(regex.find(text));
    }
    let elapsed = start.elapsed();
    let per_iter_ns = elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);

    println!("Pattern: '{pattern}' (k={edits})");
    println!("Text: {} bytes", text.len());
    println!("Time per find: {per_iter_ns:.0} ns");
    let bytes = u32::try_from(text.len()).expect("text too large");
    let throughput = f64::from(bytes) * f64::from(iterations) / elapsed.as_secs_f64() / 1_000_000.0;
    println!("Throughput: {throughput:.1} MB/s");
}
