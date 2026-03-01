use fuzzy_regex::engine::bitap::BitapMatcher;
use fuzzy_regex::engine::levenshtein::EditLimits;
use std::time::Instant;

/// Calculate elapsed time in nanoseconds per iteration (returns f64 for calculations)
fn elapsed_ns_per_iter(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(iterations)
}

/// Calculate throughput in MB/s
fn throughput_mb_per_sec(bytes: u32, start: Instant, iterations: u32) -> f64 {
    let elapsed_secs = start.elapsed().as_secs_f64();
    f64::from(bytes) * f64::from(iterations) / elapsed_secs / 1_000_000.0
}

/// Benchmark a single text size
fn benchmark_text_size(
    matcher: &BitapMatcher,
    name: &str,
    text: &str,
    iterations: u32,
) {
    let bytes = u32::try_from(text.len()).expect("text too large");
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
    }
    let ns_per_iter = elapsed_ns_per_iter(start, iterations);
    let throughput = throughput_mb_per_sec(bytes, start, iterations);
    println!(
        "  {:6} ({:>5} bytes): {:>6.0} ns/iter, {:>6.1} MB/s",
        name,
        text.len(),
        ns_per_iter,
        throughput
    );
}

/// Benchmark all text sizes for a given k value
fn benchmark_all_sizes(
    k: usize,
    pattern: &str,
    short_text: &str,
    medium_text: &str,
    long_text: &str,
    very_long_text: &str,
) {
    let impl_name = if k <= 4 {
        "(streaming_k)"
    } else {
        "(streaming_large_k)"
    };
    println!(
        "--- max_edits = {k} {impl_name} ---"
    );

    let k_u8 = u8::try_from(k).expect("k too large");
    let matcher = BitapMatcher::new(pattern, EditLimits::new(k_u8), false).unwrap();

    benchmark_text_size(&matcher, "Short", short_text, 10_000);
    benchmark_text_size(&matcher, "Medium", medium_text, 1000);
    benchmark_text_size(&matcher, "Long", long_text, 100);
    benchmark_text_size(&matcher, "VLong", very_long_text, 10);

    println!();
}

/// Benchmark streaming vs position-based for early match
fn benchmark_early_match(matcher: &BitapMatcher, text: &[u8], iterations: u32) {
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text, 0.0));
    }
    let streaming_ns = elapsed_ns_per_iter(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_at_byte_position(text, 0, 0.0));
    }
    let position_ns = elapsed_ns_per_iter(start, iterations);

    println!(
        "Text: '{}' ({} bytes)",
        std::str::from_utf8(text).unwrap(),
        text.len()
    );
    println!("  Streaming:      {streaming_ns:>6.0} ns/iter");
    println!("  Position-based: {position_ns:>6.0} ns/iter");
    println!("  Ratio: {:.2}x\n", streaming_ns / position_ns);
}

/// Benchmark streaming vs position-based for late match
fn benchmark_late_match(matcher: &BitapMatcher, text: &[u8], iterations: u32) {
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text, 0.0));
    }
    let streaming_ns = elapsed_ns_per_iter(start, iterations);

    // Position-based needs to scan from each position
    let start = Instant::now();
    for _ in 0..iterations {
        let mut found = None;
        for pos in 0..text.len() {
            if let Some(m) = matcher.find_at_byte_position(text, pos, 0.0) {
                found = Some(m);
                break;
            }
        }
        std::hint::black_box(found);
    }
    let position_scan_ns = elapsed_ns_per_iter(start, iterations);

    println!(
        "Text: '{}' ({} bytes)",
        std::str::from_utf8(text).unwrap(),
        text.len()
    );
    println!("  Streaming:           {streaming_ns:>6.0} ns/iter");
    println!("  Position-based scan: {position_scan_ns:>6.0} ns/iter");
    println!("  Streaming speedup: {:.1}x\n", position_scan_ns / streaming_ns);
}

/// Benchmark three-buffer overhead
fn benchmark_three_buffer_overhead(matcher: &BitapMatcher, text: &[u8], iterations: u32) {
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text, 0.0));
    }
    let streaming_ns = elapsed_ns_per_iter(start, iterations);
    println!("Exact match 'the' in 'the quick...': {streaming_ns:.0} ns/iter");
}

/// Benchmark stack vs heap allocation
fn benchmark_stack_vs_heap(text: &str, iterations: u32) {
    let matcher_k4 = BitapMatcher::new("hello", EditLimits::new(4), false).unwrap();
    let matcher_k5 = BitapMatcher::new("hello", EditLimits::new(5), false).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher_k4.find_first_streaming(text.as_bytes(), 0.0));
    }
    let k4_ns = elapsed_ns_per_iter(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher_k5.find_first_streaming(text.as_bytes(), 0.0));
    }
    let k5_ns = elapsed_ns_per_iter(start, iterations);

    println!("Text: {} bytes, pattern 'hello'", text.len());
    println!("  k=4 (stack arrays): {k4_ns:>6.0} ns/iter");
    println!("  k=5 (heap vectors): {k5_ns:>6.0} ns/iter");
    println!("  Heap overhead: {:.1}%", (k5_ns / k4_ns - 1.0) * 100.0);
}

fn main() {
    println!("=== Streaming Transposition Performance Benchmarks ===\n");

    // Test texts of various sizes
    let short_text = "teh quick brown fox";
    let medium_text = "teh quick brown fox ".repeat(100); // ~2KB
    let long_text = "teh quick brown fox ".repeat(1000); // ~20KB
    let very_long_text = "teh quick brown fox ".repeat(10_000); // ~200KB

    // Pattern that requires transposition to match
    let pattern = "the";

    println!("Pattern: '{pattern}' (looking for transposition match)");
    println!("Short text:  {} bytes", short_text.len());
    println!("Medium text: {} bytes", medium_text.len());
    println!("Long text:   {} bytes", long_text.len());
    println!("Very long:   {} bytes\n", very_long_text.len());

    // Benchmark with different k values
    for k in [1, 2, 3, 4, 5, 6] {
        benchmark_all_sizes(
            k,
            pattern,
            short_text,
            &medium_text,
            &long_text,
            &very_long_text,
        );
    }

    // Compare streaming vs position-based for early match
    println!("=== Streaming vs Position-based (early match) ===\n");
    let matcher = BitapMatcher::new("the", EditLimits::new(2), false).unwrap();
    let text = b"teh quick brown fox jumps over the lazy dog";
    benchmark_early_match(&matcher, text, 100_000);

    // Compare with match at end
    println!("=== Streaming vs Position-based (late match) ===\n");
    let text_with_late_match = b"xxx xxx xxx xxx xxx xxx xxx xxx xxx teh";
    benchmark_late_match(&matcher, text_with_late_match, 100_000);

    // Overhead of three-buffer vs two-buffer (no transposition case)
    println!("=== Three-buffer overhead (exact match, no transposition) ===\n");
    let matcher = BitapMatcher::new("the", EditLimits::new(2), false).unwrap();
    let text = b"the quick brown fox";
    benchmark_three_buffer_overhead(&matcher, text, 100_000);

    // Compare k=4 vs k=5 (stack vs heap)
    println!("\n=== Stack (k=4) vs Heap (k=5) allocation ===\n");
    let text = "hte quick brown fox ".repeat(100);
    benchmark_stack_vs_heap(&text, 10_000);

    println!("\n=== Benchmarks Complete ===");
}
