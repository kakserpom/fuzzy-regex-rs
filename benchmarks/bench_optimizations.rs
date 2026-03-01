//! Targeted benchmark for the 5 recent optimizations:
//! 1. `HashMap` dedup pooling (many active states)
//! 2. Binary search position lookup (long texts)
//! 3. `ActiveState` clone avoidance (patterns with deletions)
//! 4. Rc + `SmallVec` for `CaptureState` (capture groups, branching)
//! 5. Bounded edit distance (backreferences)
//!
//! Run with: cargo run --release --example `bench_optimizations`

use std::hint::black_box;
use std::time::{Duration, Instant};

use fuzzy_regex::FuzzyRegexBuilder;

const ITERATIONS: u32 = 1_000;
const WARMUP: u32 = 50;

fn bench<F: FnMut()>(name: &str, mut func: F) -> Duration {
    for _ in 0..WARMUP {
        func();
    }

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        func();
    }
    let total = start.elapsed();
    let per_iter = total / ITERATIONS;

    println!("{name}: {per_iter:?}");
    per_iter
}

fn main() {
    println!("=== Targeted Optimization Benchmarks ===\n");
    println!("Iterations: {ITERATIONS}\n");

    // Test 1: Multiple capture groups (Rc + SmallVec optimization)
    println!("--- Test 1: Multiple Capture Groups (Rc + SmallVec) ---");
    println!("Pattern with 6 capture groups");

    let text1 = "John Smith was born on 1990-05-15 in New York City and moved to Los Angeles";

    let fuzzy_regex1 = FuzzyRegexBuilder::new(
        r"((?:\w+))~1 ((?:\w+))~1 .* ((?:\d{4}))-((?:\d{2}))-((?:\d{2})) .* ((?:\w+))~1",
    )
    .build()
    .unwrap();

    bench("find (6 groups)", || {
        black_box(fuzzy_regex1.find(text1));
    });
    println!();

    // Test 2: Heavy branching (Thread clone optimization)
    println!("--- Test 2: Heavy Branching (Thread clones) ---");
    println!("Pattern with many alternations");

    let text2 = "The quick brown fox jumps over the lazy dog near the river";

    let fuzzy_regex2 = FuzzyRegexBuilder::new(
        r"((?:quick)|(?:slow)|(?:fast)|(?:rapid))~1 ((?:brown)|(?:black)|(?:white)|(?:gray))~1 ((?:fox)|(?:dog)|(?:cat)|(?:rat))~1",
    )
    .build()
    .unwrap();

    bench("find (branching)", || {
        black_box(fuzzy_regex2.find(text2));
    });
    println!();

    // Test 3: Many active states with deletions (HashMap pooling + epsilon closure)
    println!("--- Test 3: Many Active States (HashMap pooling) ---");
    println!("High edit distance pattern causing many states");

    let text3 = "abcdefghijklmnopqrstuvwxyz".repeat(4);

    let fuzzy_regex3 = FuzzyRegexBuilder::new(r"(?:abcdefgh)~4")
        .similarity(0.3)
        .build()
        .unwrap();

    bench("find_iter (many states)", || {
        let _: Vec<_> = fuzzy_regex3.find_iter(&text3).collect();
    });
    println!();

    // Test 4: Long text with many candidates (binary search)
    println!("--- Test 4: Long Text Position Lookup (binary search) ---");
    println!("1KB text with pattern");

    let text4 = "Lorem ipsum dolor sit amet consectetur adipiscing elit ".repeat(20);

    let fuzzy_regex4 = FuzzyRegexBuilder::new(r"(?:consectetur)~2")
        .build()
        .unwrap();

    bench("find_iter (1KB)", || {
        let _: Vec<_> = fuzzy_regex4.find_iter(&text4).collect();
    });
    println!();

    // Test 5: Pattern with deletions (epsilon closure optimization)
    println!("--- Test 5: Deletion-Heavy Pattern (epsilon closure) ---");
    println!("Pattern that triggers many epsilon transitions");

    let text5 = "programming programmer programmatic program programs programmed";

    let fuzzy_regex5 = FuzzyRegexBuilder::new(r"(?:programming)~3")
        .edits(3)
        .build()
        .unwrap();

    bench("find_iter (deletions)", || {
        let _: Vec<_> = fuzzy_regex5.find_iter(text5).collect();
    });
    println!();

    // Test 6: Repeated find_iter (HashMap reuse)
    println!("--- Test 6: Repeated find_iter (HashMap reuse) ---");
    println!("Multiple find_iter calls on same pattern");

    let sample_texts: Vec<String> = (0..10)
        .map(|i| format!("text{i} with saddam and hussein and more saddam here"))
        .collect();

    let fuzzy_regex6 = FuzzyRegexBuilder::new(r"(?:saddam)~2")
        .build()
        .unwrap();

    bench("find_iter x10", || {
        for sample in &sample_texts {
            let _: Vec<_> = fuzzy_regex6.find_iter(sample).collect();
        }
    });
    println!();

    // Test 7: Capture group extraction (SmallVec)
    println!("--- Test 7: Capture Extraction (SmallVec) ---");
    println!("Extract multiple captures per match");

    let text7 = "apple:red banana:yellow cherry:red grape:purple orange:orange";

    let fuzzy_regex7 = FuzzyRegexBuilder::new(r"((?:\w+))~1:((?:\w+))~1")
        .build()
        .unwrap();

    bench("captures_iter", || {
        for caps in fuzzy_regex7.captures_iter(text7) {
            black_box(caps.get(1));
            black_box(caps.get(2));
        }
    });
    println!();

    // Test 8: Very high branching factor
    println!("--- Test 8: Extreme Branching ---");
    println!("Pattern causing thread explosion");

    let text8 = "cat dog bat rat mat sat pat hat fat vat";

    // Many fuzzy alternations
    let fuzzy_regex8 = FuzzyRegexBuilder::new(
        r"(?:(?:cat)|(?:dog)|(?:bat)|(?:rat)|(?:mat)|(?:sat)|(?:pat)|(?:hat))~1",
    )
    .build()
    .unwrap();

    bench("find_iter (8 alts)", || {
        let _: Vec<_> = fuzzy_regex8.find_iter(text8).collect();
    });
}
