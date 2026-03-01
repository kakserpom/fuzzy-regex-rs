// This test directly benchmarks bitap verification without FuzzyRegex overhead

use fuzzy_regex::engine::bitap::BitapMatcher;
use fuzzy_regex::engine::levenshtein::EditLimits;
use std::time::Instant;

fn elapsed_us(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations)
}

fn main() {
    println!("=== Direct Bitap Profiling ===\n");

    let iterations: u32 = 100_000;

    // Create matchers
    let limits_fuzzy = EditLimits {
        max_edits: 1,
        max_insertions: None,
        max_deletions: None,
        max_substitutions: None,
        max_swaps: None,
    };

    let limits_exact = EditLimits {
        max_edits: 0,
        max_insertions: None,
        max_deletions: None,
        max_substitutions: None,
        max_swaps: None,
    };

    // Russian matcher
    let bitap_russian = BitapMatcher::new("Привет", limits_fuzzy.clone(), false).unwrap();
    let bitap_russian_exact = BitapMatcher::new("Привет", limits_exact.clone(), false).unwrap();

    // English matcher
    let bitap_english = BitapMatcher::new("Hello", limits_fuzzy.clone(), false).unwrap();
    let bitap_english_exact = BitapMatcher::new("Hello", limits_exact.clone(), false).unwrap();

    let text_russian = "Привет".as_bytes();
    let text_english = b"Hello";

    // Warmup
    for _ in 0..1000 {
        let _ = bitap_russian.find_at_byte_position(text_russian, 0, 0.0);
        let _ = bitap_english.find_at_byte_position(text_english, 0, 0.0);
    }

    // Test fuzzy match
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_russian.find_at_byte_position(text_russian, 0, 0.0);
    }
    let russian_fuzzy_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_english.find_at_byte_position(text_english, 0, 0.0);
    }
    let english_fuzzy_time = elapsed_us(start, iterations);

    println!("=== find_at_byte_position fuzzy (e<=1) ===");
    println!("Russian: {russian_fuzzy_time:.2} µs");
    println!("English: {english_fuzzy_time:.2} µs");
    println!("Ratio: {:.2}x\n", russian_fuzzy_time / english_fuzzy_time);

    // Test exact match
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_russian_exact.find_at_byte_position(text_russian, 0, 0.0);
    }
    let russian_exact_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_english_exact.find_at_byte_position(text_english, 0, 0.0);
    }
    let english_exact_time = elapsed_us(start, iterations);

    println!("=== find_at_byte_position exact (e<=0) ===");
    println!("Russian: {russian_exact_time:.2} µs");
    println!("English: {english_exact_time:.2} µs");
    println!("Ratio: {:.2}x\n", russian_exact_time / english_exact_time);

    // Test streaming search (single pass)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_russian.find_first_streaming(text_russian, 0.0);
    }
    let russian_stream_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_english.find_first_streaming(text_english, 0.0);
    }
    let english_stream_time = elapsed_us(start, iterations);

    println!("=== find_first_streaming (e<=1) ===");
    println!("Russian: {russian_stream_time:.2} µs");
    println!("English: {english_stream_time:.2} µs");
    println!("Ratio: {:.2}x", russian_stream_time / english_stream_time);
}
