//! Breakdown benchmark to isolate overhead sources

use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

fn main() {
    let text = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";
    let iterations: u32 = 10_000;

    let fuzzy_regex = FuzzyRegexBuilder::new("(?:saddam)~2")
        .similarity(0.6)
        .build()
        .unwrap();

    // Warmup
    for _ in 0..100 {
        let _ = fuzzy_regex.find(text);
    }

    // Measure full find
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = fuzzy_regex.find(text);
    }
    let full_time = start.elapsed();
    println!("Full find: {:?} per call", full_time / iterations);

    // Measure find_at at exact position
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = fuzzy_regex.find_at(text, 30);
    }
    let find_at_time = start.elapsed();
    println!("find_at(30): {:?} per call", find_at_time / iterations);

    // Measure find_at at position 0 (worst case - must scan to find match)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = fuzzy_regex.find_at(text, 0);
    }
    let find_at_0_time = start.elapsed();
    println!("find_at(0): {:?} per call", find_at_0_time / iterations);

    // Verify results
    println!("\nResults:");
    println!(
        "find: {:?}",
        fuzzy_regex.find(text).map(|m| (m.start(), m.end(), m.as_str()))
    );
    println!(
        "find_at(30): {:?}",
        fuzzy_regex
            .find_at(text, 30)
            .map(|m| (m.start(), m.end(), m.as_str()))
    );
    println!(
        "find_at(0): {:?}",
        fuzzy_regex
            .find_at(text, 0)
            .map(|m| (m.start(), m.end(), m.as_str()))
    );
}
