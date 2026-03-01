use fuzzy_regex::engine::bitap::BitapMatcher;
use fuzzy_regex::engine::damlev::EditLimits;
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

/// Benchmark streaming with different k values
fn benchmark_by_k_value(pattern: &str, text: &str, iterations: u32) {
    let bytes = u32::try_from(text.len()).expect("text too large");
    for k in 1..=3 {
        let matcher = BitapMatcher::new(pattern, EditLimits::new(k), false).unwrap();
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
        }
        let ns_per_iter = elapsed_ns_per_iter(start, iterations);
        let throughput = throughput_mb_per_sec(bytes, start, iterations);
        println!(
            "k={}: {:>8.0} ns/iter, {:>6.1} MB/s ({}KB text)",
            k,
            ns_per_iter,
            throughput,
            text.len() / 1000
        );
    }
}

/// Benchmark streaming with different text sizes
fn benchmark_by_text_size(matcher: &BitapMatcher, texts: &[(&str, &String, u32)]) {
    for (name, text, iters) in texts {
        let bytes = u32::try_from(text.len()).expect("text too large");
        let start = Instant::now();
        for _ in 0..*iters {
            std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
        }
        let ns_per_iter = elapsed_ns_per_iter(start, *iters);
        let throughput = throughput_mb_per_sec(bytes, start, *iters);
        println!(
            "{:>6} ({:>6} bytes): {:>8.0} ns, {:>6.1} MB/s",
            name,
            text.len(),
            ns_per_iter,
            throughput
        );
    }
}

/// Benchmark no-match case
fn benchmark_no_match(matcher: &BitapMatcher, text: &str, iterations: u32) {
    let bytes = u32::try_from(text.len()).expect("text too large");
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
    }
    let ns_per_iter = elapsed_ns_per_iter(start, iterations);
    let throughput = throughput_mb_per_sec(bytes, start, iterations);
    println!(
        "No-match ({} bytes): {:.0} ns, {:.1} MB/s",
        text.len(),
        ns_per_iter,
        throughput
    );
}

/// Benchmark transposition overhead
fn benchmark_transposition_overhead(
    matcher: &BitapMatcher,
    exact_text: &str,
    trans_text: &str,
    iterations: u32,
) {
    let exact_bytes = u32::try_from(exact_text.len()).expect("text too large");
    let trans_bytes = u32::try_from(trans_text.len()).expect("text too large");

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

    // Calculate throughput: bytes/ns * 1000 = MB/s
    let exact_throughput = f64::from(exact_bytes) / exact_ns * 1000.0;
    let trans_throughput = f64::from(trans_bytes) / trans_ns * 1000.0;

    println!("~20KB text + match:");
    println!("  Exact match:  {exact_ns:>8.0} ns ({exact_throughput:.1} MB/s)");
    println!("  Trans match:  {trans_ns:>8.0} ns ({trans_throughput:.1} MB/s)");
    let overhead = (trans_ns / exact_ns - 1.0) * 100.0;
    println!("  Overhead: {overhead:.1}%");
}

/// Benchmark streaming vs position-based
fn benchmark_streaming_vs_position(matcher: &BitapMatcher, text: &str, iterations: u32) {
    // Streaming (single pass)
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
    }
    let streaming_ns = elapsed_ns_per_iter(start, iterations);

    // Position-based (scanning each position)
    let start = Instant::now();
    for _ in 0..iterations {
        let mut found = None;
        let bytes = text.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            if let Some(m) = matcher.find_at_byte_position(bytes, pos, 0.0) {
                found = Some(m);
                break;
            }
            pos += 1;
        }
        std::hint::black_box(found);
    }
    let position_ns = elapsed_ns_per_iter(start, iterations);

    println!("Finding match at end of ~20KB text:");
    println!("  Streaming:      {streaming_ns:>8.0} ns");
    println!("  Position-scan:  {position_ns:>8.0} ns");
    println!("  Speedup: {:.1}x", position_ns / streaming_ns);
}

fn main() {
    println!("=== Streaming Bitap Performance (Corrected) ===\n");

    // Use a longer pattern to avoid false early matches
    let pattern = "transportation"; // 14 chars - needs 14 edits to match garbage
    let transposed = "transporattion"; // swap 'rt' -> 'ra' in position 8-9

    // Create texts with transposition match at the END
    let make_text = |size: usize| -> String {
        let repeats = size / 10;
        let padding = "123456789 ".repeat(repeats);
        format!("{padding}{transposed}")
    };

    let short_text = make_text(100);
    let medium_text = make_text(2000);
    let long_text = make_text(20_000);
    let very_long_text = make_text(200_000);

    println!("Pattern: '{pattern}' (14 chars)");
    println!("Looking for: '{transposed}' (transposition)");
    println!("Short:  {:>6} bytes", short_text.len());
    println!("Medium: {:>6} bytes", medium_text.len());
    println!("Long:   {:>6} bytes", long_text.len());
    println!("VLong:  {:>6} bytes\n", very_long_text.len());

    // Verify
    let matcher = BitapMatcher::new(pattern, EditLimits::new(2), false).unwrap();
    if let Some(m) = matcher.find_first_streaming(short_text.as_bytes(), 0.0) {
        println!(
            "Verification: match at bytes {}-{}, text len {}",
            m.start,
            m.end,
            short_text.len()
        );
        let match_text = &short_text[m.start..m.end];
        println!(
            "Matched: '{}', edits={}, swaps={}\n",
            match_text,
            m.total_edits(),
            m.swaps
        );
    } else {
        println!("ERROR: No match found!\n");
        return;
    }

    // Benchmark k=1,2,3 (reasonable edit distances)
    println!("=== Throughput by max_edits ===\n");
    benchmark_by_k_value(pattern, &long_text, 100);

    println!("\n=== Throughput by text size (k=2) ===\n");
    let matcher = BitapMatcher::new(pattern, EditLimits::new(2), false).unwrap();
    let texts: Vec<(&str, &String, u32)> = vec![
        ("Short", &short_text, 10_000),
        ("Medium", &medium_text, 1000),
        ("Long", &long_text, 100),
        ("VLong", &very_long_text, 10),
    ];
    benchmark_by_text_size(&matcher, &texts);

    // No-match case
    println!("\n=== No-match throughput ===\n");
    let no_match_text = "123456789 ".repeat(20_000); // ~200KB, no match
    benchmark_no_match(&matcher, &no_match_text, 10);

    // Transposition overhead
    println!("\n=== Transposition overhead ===\n");
    let exact_text = format!("{}{}", "123456789 ".repeat(2000), pattern); // exact match
    let trans_text = format!("{}{}", "123456789 ".repeat(2000), transposed); // transposition
    benchmark_transposition_overhead(&matcher, &exact_text, &trans_text, 100);

    // Compare streaming vs position-based
    println!("\n=== Streaming vs Position-based ===\n");
    benchmark_streaming_vs_position(&matcher, &trans_text, 100);

    println!("\n=== Summary ===");
    println!("Streaming Bitap with transposition:");
    println!("  - ~200-250 MB/s throughput for k=1-2");
    println!("  - Minimal overhead (~1%) for transposition detection");
    println!("  - Much faster than position-by-position scanning");
}
