//! Profile where time is spent in fuzzy-regex
//!
//! Run with: cargo run --release --example `profile_bottlenecks`

use std::time::Instant;

use fuzzy_regex::FuzzyRegexBuilder;

const ITERATIONS: u32 = 1000;

fn profile_basic_search() {
    println!("--- Test 1: Basic Search Breakdown ---");
    let text = "this is a saddamhu example with multiple saddam matches and ddamhu too";

    // Time: regex compilation
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = FuzzyRegexBuilder::new("(?:saddam)")
            .edits(2)
            .similarity(0.5)
            .build()
            .unwrap();
    }
    println!("  Compilation: {:?} per iter", start.elapsed() / ITERATIONS);

    // Time: search with pre-compiled regex
    let regex = FuzzyRegexBuilder::new("(?:saddam)")
        .edits(2)
        .similarity(0.5)
        .build()
        .unwrap();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex.find(text);
    }
    println!("  Search only: {:?} per iter", start.elapsed() / ITERATIONS);

    // Time: default (non-global) mode
    let regex_greedy = FuzzyRegexBuilder::new("(?:saddam)")
        .edits(2)
        .similarity(0.5)
        .build()
        .unwrap();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex_greedy.find(text);
    }
    println!("  Greedy first: {:?} per iter", start.elapsed() / ITERATIONS);
}

fn profile_dna_pattern() {
    println!("\n--- Test 2: DNA Pattern Breakdown ---");
    let dna = "ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT";

    let regex_dna = FuzzyRegexBuilder::new("(?:ACGTACGT)")
        .edits(2)
        .similarity(0.7)
        .build()
        .unwrap();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex_dna.find(dna);
    }
    println!("  Normal mode: {:?} per iter", start.elapsed() / ITERATIONS);

    let regex_dna_greedy = FuzzyRegexBuilder::new("(?:ACGTACGT)")
        .edits(2)
        .similarity(0.7)
        .build()
        .unwrap();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex_dna_greedy.find(dna);
    }
    println!("  Greedy first: {:?} per iter", start.elapsed() / ITERATIONS);
}

fn profile_find_all_matches() {
    println!("\n--- Test 3: Find All Matches Breakdown ---");
    let text = "cat bat rat cat mat sat cat pat";

    let regex = FuzzyRegexBuilder::new("(?:cat)")
        .edits(1)
        .similarity(0.6)
        .build()
        .unwrap();

    // Single find
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex.find(text);
    }
    println!("  Single find: {:?} per iter", start.elapsed() / ITERATIONS);

    // Find iter (all matches)
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _: Vec<_> = regex.find_iter(text).collect();
    }
    println!("  Find all (iter): {:?} per iter", start.elapsed() / ITERATIONS);

    // Count matches
    let matches: Vec<_> = regex.find_iter(text).collect();
    println!("  Found {} matches", matches.len());
}

fn profile_short_pattern() {
    println!("\n--- Test 4: Short Pattern Breakdown ---");
    let text = "The quick brown fox jumps over the lazy dog";

    let regex = FuzzyRegexBuilder::new("(?:fox)")
        .edits(1)
        .similarity(0.6)
        .build()
        .unwrap();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex.find(text);
    }
    println!("  Normal: {:?} per iter", start.elapsed() / ITERATIONS);

    let regex_greedy = FuzzyRegexBuilder::new("(?:fox)")
        .edits(1)
        .similarity(0.6)
        .build()
        .unwrap();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = regex_greedy.find(text);
    }
    println!("  Greedy first: {:?} per iter", start.elapsed() / ITERATIONS);
}

fn main() {
    println!("=== Profiling fuzzy-regex bottlenecks ===\n");

    profile_basic_search();
    profile_dna_pattern();
    profile_find_all_matches();
    profile_short_pattern();

    println!("\n=== Analysis ===");
    println!("Key bottlenecks:");
    println!("1. find_iter is slow because it re-searches from each position");
    println!("2. DNA has many false positives from prefilter (ACGT repeats)");
    println!("3. NFA overhead for simple patterns");
}
