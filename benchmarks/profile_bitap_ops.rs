//! Profile individual bitap operations for Russian

use std::time::Instant;

use fuzzy_regex::engine::bitap::BitapMatcher;
use fuzzy_regex::engine::levenshtein::EditLimits;

/// Calculate elapsed time in nanoseconds per iteration
fn elapsed_ns(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(iterations)
}

fn main() {
    let iterations = 100_000;

    let limits = EditLimits {
        max_edits: 1,
        max_insertions: None,
        max_deletions: None,
        max_substitutions: None,
        max_swaps: None,
    };

    // Russian pattern (6 chars, 12 bytes)
    let bitap_russian = BitapMatcher::new("Привет", limits.clone(), false).unwrap();
    let text_russian = "Привет мир!".as_bytes();

    // English pattern (5 chars, 5 bytes)
    let bitap_english = BitapMatcher::new("Hello", limits.clone(), false).unwrap();
    let text_english = b"Hello world!";

    // Warmup
    for _ in 0..1000 {
        let _ = bitap_russian.find_at_byte_position(text_russian, 0, 0.0);
        let _ = bitap_english.find_at_byte_position(text_english, 0, 0.0);
    }

    // Profile find_at_byte_position
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_russian.find_at_byte_position(text_russian, 0, 0.0);
    }
    let russian_bitap = elapsed_ns(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_english.find_at_byte_position(text_english, 0, 0.0);
    }
    let english_bitap = elapsed_ns(start, iterations);

    println!("=== BitapMatcher.find_at_byte_position() ===");
    println!("Russian: {russian_bitap:.1} ns");
    println!("English: {english_bitap:.1} ns");
    println!("Ratio: {:.2}x", russian_bitap / english_bitap);

    // Pattern info
    println!("\n=== Pattern Info ===");
    println!(
        "Russian pattern: 'Привет' - 6 chars, {} bytes",
        "Привет".len()
    );
    println!("English pattern: 'Hello' - 5 chars, {} bytes", "Hello".len());
    println!(
        "Russian text: 'Привет мир!' - {} chars, {} bytes",
        "Привет мир!".chars().count(),
        "Привет мир!".len()
    );
    println!(
        "English text: 'Hello world!' - {} chars, {} bytes",
        "Hello world!".chars().count(),
        "Hello world!".len()
    );

    // Breakdown: pure state update vs full operation
    // We can't easily isolate this without modifying the crate, but we can estimate
    // based on pattern length and edit distance

    // For Russian: 6 chars * 2 bytes = 12 bytes to decode + mask lookup
    // For English: 5 chars = 5 bytes, direct mask lookup
    // Overhead = UTF-8 decode + mask lookup difference + edit breakdown compute
}
