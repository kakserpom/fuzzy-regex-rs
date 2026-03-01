//! Benchmark for multi-position parallel SIMD search.

use std::time::Instant;

fn main() {
    println!("=== Multi-Position Parallel Benchmark ===\n");

    #[cfg(target_arch = "aarch64")]
    println!("Architecture: ARM64 (NEON)\n");

    #[cfg(target_arch = "x86_64")]
    println!("Architecture: x86_64 (AVX2)\n");

    // Create test data
    let text = "The quick brown fox jumps over the lazy dog. ".repeat(100);
    let text_bytes = text.as_bytes();

    // Multiple candidate positions (simulating prefilter results)
    let positions: Vec<usize> = (0..text_bytes.len())
        .filter(|&i| i % 10 == 0) // Every 10th position
        .collect();

    println!("Text size: {} bytes", text_bytes.len());
    println!("Candidate positions: {}\n", positions.len());

    let iterations = 10_000u32;

    // Test 1: k=0 exact match (uses multi-position SIMD)
    println!("--- Test 1: k=0 Exact Match ---");
    bench_pattern("fox", 0, &text, &positions, iterations);

    // Test 2: k=1 fuzzy match
    println!("\n--- Test 2: k=1 Fuzzy Match ---");
    bench_pattern("fox", 1, &text, &positions, iterations);

    // Test 3: Longer pattern k=0
    println!("\n--- Test 3: Longer Pattern k=0 ---");
    bench_pattern("quick brown", 0, &text, &positions, iterations);

    // Test 4: Many positions, pattern near end
    println!("\n--- Test 4: Pattern Near End ---");
    bench_pattern("dog", 0, &text, &positions, iterations);

    // Test 5: Non-matching pattern
    println!("\n--- Test 5: Non-Matching Pattern ---");
    bench_pattern("xyz", 0, &text, &positions, iterations);

    // Comparison: sequential vs parallel (direct Bitap access)
    println!("\n=== Direct Bitap Comparison ===");
    compare_sequential_vs_parallel(&text, &positions);
}

fn bench_pattern(pattern: &str, edits: u8, text: &str, _positions: &[usize], iterations: u32) {
    use fuzzy_regex::FuzzyRegexBuilder;

    let regex = FuzzyRegexBuilder::new(&format!("(?:{pattern})"))
        .edits(edits)
        .similarity(0.5)
        .build()
        .unwrap();

    // Warmup
    for _ in 0..100 {
        std::hint::black_box(regex.find(text));
    }

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(regex.find(text));
    }
    let elapsed = start.elapsed();
    let per_iter_ns = elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);

    println!("Pattern: '{pattern}' (k={edits})");
    println!("Time per find: {per_iter_ns:.0} ns");
    // Throughput calculation - text is small enough that u32 suffices
    let text_len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    let throughput = f64::from(text_len) * f64::from(iterations) / elapsed.as_secs_f64() / 1_000_000.0;
    println!("Throughput: {throughput:.1} MB/s");
}

fn compare_sequential_vs_parallel(text: &str, positions: &[usize]) {
    use fuzzy_regex::engine::bitap::BitapMatcher;
    use fuzzy_regex::engine::levenshtein::EditLimits;

    let pattern = "fox";
    let text_bytes = text.as_bytes();

    // Create bitap matcher for k=0
    let bitap = BitapMatcher::new(pattern, EditLimits::new(0), false).unwrap();

    let iterations = 10_000u32;
    let threshold = 1.0;

    // Warmup
    for _ in 0..100 {
        for &pos in positions.iter().take(10) {
            std::hint::black_box(bitap.find_at_byte_position(text_bytes, pos, threshold));
        }
    }

    // Sequential: check each position one at a time
    println!("\n--- Sequential (find_at_byte_position) ---");
    let start = Instant::now();
    for _ in 0..iterations {
        for &pos in positions {
            if let Some(_m) = bitap.find_at_byte_position(text_bytes, pos, threshold) {
                break;
            }
        }
    }
    let seq_elapsed = start.elapsed();
    let seq_per_iter_ns = seq_elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);
    println!("Time per search: {seq_per_iter_ns:.0} ns");

    // Parallel: use find_at_positions_parallel
    println!("\n--- Parallel (find_at_positions_parallel) ---");
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(bitap.find_at_positions_parallel(text_bytes, positions, threshold));
    }
    let par_elapsed = start.elapsed();
    let par_per_iter_ns = par_elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);
    println!("Time per search: {par_per_iter_ns:.0} ns");

    let speedup = seq_per_iter_ns / par_per_iter_ns;
    println!("\nSpeedup: {speedup:.2}x");
}
