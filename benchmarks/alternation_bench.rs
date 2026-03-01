//! Benchmark for alternation (multi-pattern) fuzzy matching
//!
//! Run with: cargo run --release --example `alternation_bench`

use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

fn bench<F: FnMut() -> bool>(name: &str, iterations: u32, mut f: F) -> f64 {
    // Warmup
    for _ in 0..50 {
        let _ = f();
    }

    let start = Instant::now();
    let mut found_count = 0u32;
    for _ in 0..iterations {
        if f() {
            found_count += 1;
        }
    }
    let elapsed = start.elapsed();

    let per_iter_us = elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations);
    println!("{name:50} {per_iter_us:>10.2} us/iter  (found: {found_count})");
    per_iter_us
}

fn run_alternation_tests() {
    println!("Alternation (Multi-Pattern) Fuzzy Benchmark");
    println!("============================================\n");

    // Test text
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    let long_text = medium_text.repeat(20);
    let dna: String = (0..1000).map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' }).collect();

    // Test 1: Simple alternation (2 patterns)
    println!("Test 1: 2 alternatives");
    let re1 = FuzzyRegexBuilder::new("(?:quick|lazy){e<=1}")
        .build()
        .unwrap();
    bench("  (quick|lazy) e<=1 in short", 10_000, || re1.find(short_text).is_some());

    // Test 2: 3 alternatives
    println!("\nTest 2: 3 alternatives");
    let re2 = FuzzyRegexBuilder::new("(?:quick|brown|lazy){e<=1}")
        .build()
        .unwrap();
    bench("  (quick|brown|lazy) e<=1 in short", 10_000, || re2.find(short_text).is_some());

    // Test 3: 5 alternatives
    println!("\nTest 3: 5 alternatives");
    let re3 = FuzzyRegexBuilder::new("(?:quick|brown|fox|jumps|lazy){e<=1}")
        .build()
        .unwrap();
    bench("  (quick|brown|fox|jumps|lazy) e<=1 short", 10_000, || re3.find(short_text).is_some());

    // Test 4: Alternation in medium text
    println!("\nTest 4: Alternation in medium text ({} bytes)", medium_text.len());
    let re4 = FuzzyRegexBuilder::new("(?:Lorem|ipsum|dolor){e<=2}")
        .build()
        .unwrap();
    bench("  (Lorem|ipsum|dolor) e<=2", 10_000, || re4.find(medium_text).is_some());

    // Test 5: Alternation in long text
    println!("\nTest 5: Alternation in long text ({} bytes)", long_text.len());
    let re5 = FuzzyRegexBuilder::new("(?:Lorem|ipsum|dolor){e<=2}")
        .build()
        .unwrap();
    bench("  (Lorem|ipsum|dolor) e<=2", 1_000, || re5.find(&long_text).is_some());

    // Test 6: DNA motifs (3 alternatives)
    println!("\nTest 6: DNA motifs (3 alternatives)");
    let re6 = FuzzyRegexBuilder::new("(?:ACGTACGT|TGCATGCA|GGCCGGCC){e<=2}")
        .build()
        .unwrap();
    bench("  DNA 3 motifs e<=2", 10_000, || re6.find(&dna).is_some());

    // Test 7: No match with alternation (using patterns that definitely won't match)
    println!("\nTest 7: No match (alternation)");
    let re7 = FuzzyRegexBuilder::new("(?:zzzzz|xxxxx|yyyyy){e<=1}")
        .build()
        .unwrap();
    bench("  (zzzzz|xxxxx|yyyyy) no match short", 10_000, || re7.find(short_text).is_some());
    bench("  (zzzzz|xxxxx|yyyyy) no match medium", 1_000, || re7.find(medium_text).is_some());

    // Test 8: Longer alternatives
    println!("\nTest 8: Longer alternatives");
    let re8 = FuzzyRegexBuilder::new("(?:consectetur|adipiscing|exercitation){e<=2}")
        .build()
        .unwrap();
    bench("  3 long words e<=2 in medium", 10_000, || re8.find(medium_text).is_some());

    // Test 9: 10 alternatives
    println!("\nTest 9: 10 alternatives");
    let re9 = FuzzyRegexBuilder::new("(?:the|quick|brown|fox|jumps|over|lazy|dog|lorem|ipsum){e<=1}")
        .build()
        .unwrap();
    bench("  10 alternatives e<=1 in short", 10_000, || re9.find(short_text).is_some());

    // Test 10: Compare single vs alternation
    println!("\nTest 10: Single pattern vs alternation");
    let re_single = FuzzyRegexBuilder::new("(?:quick){e<=1}")
        .build()
        .unwrap();
    let re_alt2 = FuzzyRegexBuilder::new("(?:quick|xxxxx){e<=1}")
        .build()
        .unwrap();
    let re_alt5 = FuzzyRegexBuilder::new("(?:quick|xxxxx|yyyyy|zzzzz|wwwww){e<=1}")
        .build()
        .unwrap();

    bench("  single: quick e<=1", 10_000, || re_single.find(short_text).is_some());
    bench("  2 alts: (quick|xxxxx) e<=1", 10_000, || re_alt2.find(short_text).is_some());
    bench("  5 alts: (quick|xxxxx|...) e<=1", 10_000, || re_alt5.find(short_text).is_some());

    println!("\nDone!");
}

fn main() {
    run_alternation_tests();
}
