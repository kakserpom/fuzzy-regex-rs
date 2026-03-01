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
        "  {:6} ({:>6} bytes): {:>7.0} ns/iter, {:>6.1} MB/s",
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
    println!("--- max_edits = {k} {impl_name} ---");

    let k_u8 = u8::try_from(k).expect("k too large");
    let matcher = BitapMatcher::new(pattern, EditLimits::new(k_u8), false).unwrap();

    benchmark_text_size(&matcher, "Short", short_text, 10_000);
    benchmark_text_size(&matcher, "Medium", medium_text, 1000);
    benchmark_text_size(&matcher, "Long", long_text, 100);
    benchmark_text_size(&matcher, "VLong", very_long_text, 10);

    println!();
}

/// Benchmark no-match performance
fn benchmark_no_match(pattern: &str, text: &str, iterations: u32) {
    let bytes = u32::try_from(text.len()).expect("text too large");
    let matcher = BitapMatcher::new(pattern, EditLimits::new(2), false).unwrap();

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
    }
    let ns_per_iter = elapsed_ns_per_iter(start, iterations);
    let throughput = throughput_mb_per_sec(bytes, start, iterations);
    println!(
        "No-match ({} bytes): {:.0} ns/iter, {:.1} MB/s\n",
        text.len(),
        ns_per_iter,
        throughput
    );
}

/// Benchmark transposition overhead
fn benchmark_transposition_overhead(
    pattern: &str,
    text_with_match: &str,
    text_with_trans: &str,
    text_no_match: &str,
    iterations: u32,
) {
    let matcher = BitapMatcher::new(pattern, EditLimits::new(2), false).unwrap();

    // Exact match
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text_with_match.as_bytes(), 0.0));
    }
    let exact_ns = elapsed_ns_per_iter(start, iterations);

    // Transposition match
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text_with_trans.as_bytes(), 0.0));
    }
    let trans_ns = elapsed_ns_per_iter(start, iterations);

    // No match (must scan all)
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text_no_match.as_bytes(), 0.0));
    }
    let nomatch_ns = elapsed_ns_per_iter(start, iterations);

    println!("20KB text:");
    println!("  Exact match at end:  {exact_ns:>7.0} ns");
    println!("  Trans match at end:  {trans_ns:>7.0} ns");
    println!("  No match (full scan):{nomatch_ns:>7.0} ns");
    let overhead = (trans_ns / exact_ns - 1.0) * 100.0;
    println!("  Trans overhead: {overhead:.1}%");
}

fn main() {
    println!("=== Realistic Streaming Performance (match at END of text) ===\n");

    // Create texts with match at the END
    let make_text = |size: usize| -> String {
        let padding = "xxx ".repeat(size / 4);
        format!("{padding}teh") // "teh" at the end
    };

    let short_text = make_text(100);
    let medium_text = make_text(2000);
    let long_text = make_text(20_000);
    let very_long_text = make_text(200_000);

    let pattern = "the";
    println!("Pattern: '{pattern}' (transposition match at END of text)");
    println!("Short text:  {} bytes", short_text.len());
    println!("Medium text: {} bytes", medium_text.len());
    println!("Long text:   {} bytes", long_text.len());
    println!("Very long:   {} bytes\n", very_long_text.len());

    // Verify matches are at the end
    let matcher = BitapMatcher::new(pattern, EditLimits::new(1), false).unwrap();
    let m = matcher
        .find_first_streaming(short_text.as_bytes(), 0.0)
        .unwrap();
    println!(
        "Verification: match found at byte {}-{} (text len {})\n",
        m.start,
        m.end,
        short_text.len()
    );

    for k in [1, 2, 3, 4, 5, 6] {
        benchmark_all_sizes(
            k,
            pattern,
            &short_text,
            &medium_text,
            &long_text,
            &very_long_text,
        );
    }

    // Compare with no-match case
    println!("=== No-match performance (must scan entire text) ===\n");
    let no_match_text = "xxx ".repeat(50_000); // 200KB, no match
    benchmark_no_match("the", &no_match_text, 10);

    // Transposition cost comparison
    println!("=== Transposition overhead ===\n");
    let text = "xxx ".repeat(5000); // 20KB
    let text_with_match = format!("{}the", &text[..text.len() - 3]); // exact match at end
    let text_with_trans = format!("{}teh", &text[..text.len() - 3]); // transposition at end

    benchmark_transposition_overhead("the", &text_with_match, &text_with_trans, &text, 100);

    println!("\n=== Benchmarks Complete ===");
}
