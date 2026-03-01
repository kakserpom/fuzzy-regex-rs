//! Benchmark for state rotation optimization.

use fuzzy_regex::engine::bitap::BitapMatcher;
use fuzzy_regex::engine::damlev::EditLimits;
use std::time::Instant;

fn main() {
    println!("=== State Rotation Benchmark ===\n");

    // Test different text sizes
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let medium_text = short_text.repeat(100);
    let long_text = short_text.repeat(1000);

    let iterations: u32 = 10_000;

    // Test find_first
    println!("--- find_first ---");
    bench_find_first("fox", 1, short_text, iterations * 10);
    bench_find_first("fox", 1, &medium_text, iterations);
    bench_find_first("fox", 1, &long_text, iterations / 10);

    // Test find_all
    println!("\n--- find_all ---");
    bench_find_all("fox", 1, short_text, iterations * 10);
    bench_find_all("fox", 1, &medium_text, iterations);
    bench_find_all("the", 1, &medium_text, iterations); // Many matches

    // Test with different k values
    println!("\n--- Different k values (medium text) ---");
    for k in 0..=4 {
        bench_find_first("quick", k, &medium_text, iterations);
    }

    // Test using FuzzyRegex API
    println!("\n--- FuzzyRegex API ---");
    bench_regex("fox", 1, short_text, iterations * 10);
    bench_regex("fox", 1, &medium_text, iterations);
    bench_regex("fox", 1, &long_text, iterations / 10);
}

fn bench_find_first(pattern: &str, k: u8, text: &str, iterations: u32) {
    let bitap = BitapMatcher::new(pattern, EditLimits::new(k), false).unwrap();

    // Warmup
    for _ in 0..100 {
        std::hint::black_box(bitap.find_first(text, 0.5));
    }

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(bitap.find_first(text, 0.5));
    }
    let elapsed = start.elapsed();
    let per_iter_ns = elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);
    let bytes = u32::try_from(text.len()).expect("text too large");
    let throughput = f64::from(bytes) * f64::from(iterations) / elapsed.as_secs_f64() / 1_000_000.0;

    println!(
        "find_first('{}', k={}) on {} bytes: {:.0} ns/iter, {:.1} MB/s",
        pattern,
        k,
        text.len(),
        per_iter_ns,
        throughput
    );
}

fn bench_find_all(pattern: &str, k: u8, text: &str, iterations: u32) {
    let bitap = BitapMatcher::new(pattern, EditLimits::new(k), false).unwrap();

    // Warmup
    for _ in 0..100 {
        std::hint::black_box(bitap.find_all(text, 0.5));
    }

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(bitap.find_all(text, 0.5));
    }
    let elapsed = start.elapsed();
    let per_iter_ns = elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations);
    let bytes = u32::try_from(text.len()).expect("text too large");
    let throughput = f64::from(bytes) * f64::from(iterations) / elapsed.as_secs_f64() / 1_000_000.0;

    println!(
        "find_all('{}', k={}) on {} bytes: {:.0} ns/iter, {:.1} MB/s",
        pattern,
        k,
        text.len(),
        per_iter_ns,
        throughput
    );
}

fn bench_regex(pattern: &str, k: u8, text: &str, iterations: u32) {
    use fuzzy_regex::FuzzyRegexBuilder;

    let regex = FuzzyRegexBuilder::new(&format!("(?:{pattern})"))
        .edits(k)
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
    let bytes = u32::try_from(text.len()).expect("text too large");
    let throughput = f64::from(bytes) * f64::from(iterations) / elapsed.as_secs_f64() / 1_000_000.0;

    println!(
        "FuzzyRegex('{}', k={}) on {} bytes: {:.0} ns/iter, {:.1} MB/s",
        pattern,
        k,
        text.len(),
        per_iter_ns,
        throughput
    );
}
