//! Benchmark `find_at_byte_position` for a single position

use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

fn main() {
    let text = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";

    // Build regex to get access to internal timing
    let fuzzy_regex = FuzzyRegexBuilder::new("(?:saddam)~2")
        .similarity(0.6)
        .build()
        .unwrap();

    // Test: time just the find
    let iterations: u32 = 10_000;

    // Warmup
    for _ in 0..100 {
        let _ = fuzzy_regex.find(text);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = fuzzy_regex.find(text);
    }
    let total = start.elapsed();
    println!("Total find time: {:?} per iteration", total / iterations);

    // For comparison, just scan the text for 's'
    let text_bytes = text.as_bytes();
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = memchr::memchr(b's', text_bytes);
    }
    let memchr_time = start.elapsed();
    println!("memchr time: {:?} per iteration", memchr_time / iterations);
}
