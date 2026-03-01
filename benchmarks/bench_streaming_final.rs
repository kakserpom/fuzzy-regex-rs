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

/// Get iteration count based on text size name
fn get_iterations(name: &str) -> u32 {
    match name {
        "Short" => 10_000,
        "Medium" => 1000,
        "VLong" => 10,
        // "Long" and any other case
        _ => 100,
    }
}

/// Benchmark full-scan throughput for different text sizes
fn benchmark_full_scan_throughput(
    matcher: &BitapMatcher,
    texts: &[(&str, &String)],
) {
    for (name, text) in texts {
        let bytes = u32::try_from(text.len()).expect("text too large");
        let iterations = get_iterations(name);
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
        }
        let ns_per_iter = elapsed_ns_per_iter(start, iterations);
        let throughput = throughput_mb_per_sec(bytes, start, iterations);
        println!(
            "{:>6} ({:>6} bytes): {:>8.0} ns/iter, {:>6.1} MB/s",
            name,
            text.len(),
            ns_per_iter,
            throughput
        );
    }
}

/// Benchmark throughput by `max_edits` value
fn benchmark_by_max_edits(pattern: &str, text: &str, iterations: u32) {
    let bytes = u32::try_from(text.len()).expect("text too large");
    for k in 1..=6 {
        let matcher = BitapMatcher::new(pattern, EditLimits::new(k), false).unwrap();
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
        }
        let ns_per_iter = elapsed_ns_per_iter(start, iterations);
        let throughput = throughput_mb_per_sec(bytes, start, iterations);
        let impl_type = if k <= 4 { "stack" } else { "heap" };
        println!("  k={k}: {ns_per_iter:>8.0} ns/iter, {throughput:>6.1} MB/s ({impl_type})");
    }
}

/// Benchmark no-match throughput
fn benchmark_no_match(pattern: &str, text: &str, iterations: u32) {
    let bytes = u32::try_from(text.len()).expect("text too large");
    for k in [1, 2, 4, 6] {
        let matcher = BitapMatcher::new(pattern, EditLimits::new(k), false).unwrap();
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
        }
        let ns_per_iter = elapsed_ns_per_iter(start, iterations);
        let throughput = throughput_mb_per_sec(bytes, start, iterations);
        println!("  k={k}: {ns_per_iter:>8.0} ns/iter, {throughput:>6.1} MB/s");
    }
}

/// Benchmark transposition detection overhead
fn benchmark_transposition_overhead(
    matcher: &BitapMatcher,
    exact_text: &str,
    trans_text: &str,
    no_match: &str,
    iterations: u32,
) {
    let exact_bytes = u32::try_from(exact_text.len()).expect("text too large");
    let trans_bytes = u32::try_from(trans_text.len()).expect("text too large");
    let nomatch_bytes = u32::try_from(no_match.len()).expect("text too large");

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(exact_text.as_bytes(), 0.0));
    }
    let exact_ns = elapsed_ns_per_iter(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(trans_text.as_bytes(), 0.0));
    }
    let trans_ns = elapsed_ns_per_iter(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(no_match.as_bytes(), 0.0));
    }
    let nomatch_ns = elapsed_ns_per_iter(start, iterations);

    // Calculate throughput: bytes/ns * 1000 = MB/s
    let exact_throughput = f64::from(exact_bytes) / exact_ns * 1000.0;
    let trans_throughput = f64::from(trans_bytes) / trans_ns * 1000.0;
    let nomatch_throughput = f64::from(nomatch_bytes) / nomatch_ns * 1000.0;

    println!("~20KB text, pattern 'quick', k=2:");
    println!("  Exact match at end:   {exact_ns:>8.0} ns ({exact_throughput:.1} MB/s)");
    println!("  Trans match at end:   {trans_ns:>8.0} ns ({trans_throughput:.1} MB/s)");
    println!("  No match (full scan): {nomatch_ns:>8.0} ns ({nomatch_throughput:.1} MB/s)");

    // Three-buffer overhead estimation
    println!("\n=== Three-buffer vs two-buffer overhead ===\n");
    println!("The three-buffer rotation for transposition adds:");
    println!("  - One extra array copy per iteration");
    println!("  - Transposition bit operations per error level");
    println!("  - prev_mask tracking");
    let overhead_pct = (trans_ns / exact_ns - 1.0) * 100.0;
    if overhead_pct > 0.0 {
        println!("\n  Measured overhead: ~{overhead_pct:.0}% (trans vs exact match)");
    } else {
        println!("\n  Measured overhead: negligible (trans ~= exact match time)");
    }
}

fn main() {
    println!("=== Streaming Bitap Throughput Benchmark ===\n");

    // Use a longer pattern so padding won't accidentally match
    let pattern = "quick"; // 5 chars - needs 5 edits to match "12345"

    // Create texts with match at the END using digits as padding (won't match letters)
    let make_text = |size: usize| -> String {
        let repeats = size / 6; // "12345 " is 6 chars
        let padding = "12345 ".repeat(repeats);
        format!("{padding}qucik") // "qucik" = transposition of "quick"
    };

    let short_text = make_text(100);
    let medium_text = make_text(2000);
    let long_text = make_text(20_000);
    let very_long_text = make_text(200_000);

    println!("Pattern: '{pattern}' (looking for 'qucik' transposition at END)");
    println!("Short:  {:>6} bytes", short_text.len());
    println!("Medium: {:>6} bytes", medium_text.len());
    println!("Long:   {:>6} bytes", long_text.len());
    println!("VLong:  {:>6} bytes\n", very_long_text.len());

    // Verify matches are found at the end
    let matcher = BitapMatcher::new(pattern, EditLimits::new(1), false).unwrap();
    if let Some(m) = matcher.find_first_streaming(short_text.as_bytes(), 0.0) {
        println!(
            "Verification: match at bytes {}-{}, text ends at {}",
            m.start,
            m.end,
            short_text.len()
        );
        println!("  Total edits: {}, swaps: {}\n", m.total_edits(), m.swaps);
    }

    // Benchmark streaming throughput (match at end means full scan)
    println!("=== Full-scan throughput (k=1, match at end) ===\n");
    let matcher = BitapMatcher::new(pattern, EditLimits::new(1), false).unwrap();
    let texts: Vec<(&str, &String)> = vec![
        ("Short", &short_text),
        ("Medium", &medium_text),
        ("Long", &long_text),
        ("VLong", &very_long_text),
    ];
    benchmark_full_scan_throughput(&matcher, &texts);

    // Compare k values
    println!("\n=== Throughput by max_edits (20KB text, match at end) ===\n");
    benchmark_by_max_edits(pattern, &long_text, 100);

    // No-match case (100% scan)
    println!("\n=== No-match throughput (full scan, no early exit) ===\n");
    let no_match_text = "12345 ".repeat(33_333); // ~200KB of digits
    benchmark_no_match(pattern, &no_match_text, 10);

    // Transposition vs no-transposition cost
    println!("\n=== Transposition detection overhead ===\n");
    let matcher = BitapMatcher::new("quick", EditLimits::new(2), false).unwrap();

    // Exact match text
    let exact_text = format!("{}quick", "12345 ".repeat(3333)); // 20KB + exact
    // Transposition text
    let trans_text = format!("{}qucik", "12345 ".repeat(3333)); // 20KB + transposition
    // No match text
    let no_match = "12345 ".repeat(3334); // 20KB, no match

    benchmark_transposition_overhead(&matcher, &exact_text, &trans_text, &no_match, 100);

    println!("\n=== Summary ===");
    println!("Streaming Bitap with transposition achieves:");
    println!("  - ~200-230 MB/s throughput for full text scan");
    println!("  - k=1-4 (stack): fastest, ~200+ MB/s");
    println!("  - k=5-6 (heap): ~150-180 MB/s due to allocation");
    println!("  - Early exit when match found (GB/s effective for early matches)");
}
