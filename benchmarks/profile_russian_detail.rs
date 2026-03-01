//! Detailed profiling of Russian pattern matching

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

    // Russian pattern
    let bitap_russian = BitapMatcher::new("Привет", limits.clone(), false).unwrap();
    let text_russian = "Привет мир!".as_bytes();

    // English pattern
    let bitap_english = BitapMatcher::new("Hello", limits.clone(), false).unwrap();
    let text_english = b"Hello world!";

    // Warmup
    for _ in 0..1000 {
        let _ = bitap_russian.find_at_byte_position(text_russian, 0, 0.0);
        let _ = bitap_english.find_at_byte_position(text_english, 0, 0.0);
    }

    // Profile Russian
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_russian.find_at_byte_position(text_russian, 0, 0.0);
    }
    let russian_time = elapsed_ns(start, iterations);

    // Profile English
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bitap_english.find_at_byte_position(text_english, 0, 0.0);
    }
    let english_time = elapsed_ns(start, iterations);

    println!("=== Direct BitapMatcher Performance ===");
    println!("Russian: {russian_time:.1} ns");
    println!("English: {english_time:.1} ns");
    println!("Ratio: {:.2}x", russian_time / english_time);

    // Profile individual operations
    println!("\n=== Operation Breakdown ===");

    // Test mask lookup time for Russian chars
    let start = Instant::now();
    let mut sum = 0u64;
    for _ in 0..iterations {
        // Simulate what happens in the loop for 6 Russian characters
        for byte_pair in [
            (0xD0u8, 0x9Fu8),
            (0xD1, 0x80),
            (0xD0, 0xB8),
            (0xD0, 0xB2),
            (0xD0, 0xB5),
            (0xD1, 0x82),
        ] {
            let codepoint =
                ((u32::from(byte_pair.0) & 0x1F) << 6) | (u32::from(byte_pair.1) & 0x3F);
            sum = sum.wrapping_add(u64::from(codepoint));
        }
    }
    let decode_time = elapsed_ns(start, iterations);
    println!("UTF-8 decode (6 chars): {decode_time:.1} ns (sum={sum})");

    // Test char creation time
    let start = Instant::now();
    let mut sum = 0u64;
    for _ in 0..iterations {
        for byte_pair in [
            (0xD0u8, 0x9Fu8),
            (0xD1, 0x80),
            (0xD0, 0xB8),
            (0xD0, 0xB2),
            (0xD0, 0xB5),
            (0xD1, 0x82),
        ] {
            let codepoint =
                ((u32::from(byte_pair.0) & 0x1F) << 6) | (u32::from(byte_pair.1) & 0x3F);
            let ch = unsafe { char::from_u32_unchecked(codepoint) };
            sum = sum.wrapping_add(ch as u64);
        }
    }
    let char_time = elapsed_ns(start, iterations);
    println!("char creation (6 chars): {char_time:.1} ns (sum={sum})");

    println!("\nRussian text bytes: {text_russian:?}");
    println!("Pattern bytes: {:?}", "Привет".as_bytes());
}
