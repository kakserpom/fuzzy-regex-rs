//! Standalone benchmark for fuzzy-regex.
//!
//! Run with: cargo run --release --example benchmark
//! Run with mimalloc: cargo run --release --example benchmark --features mimalloc
//!
//! Compare results with fuzzy-aho-corasick by running both benchmarks
//! and comparing the output.

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: fuzzy_regex::MiMalloc = fuzzy_regex::MiMalloc;

use std::hint::black_box;
use std::time::{Duration, Instant};

use fuzzy_regex::FuzzyRegexBuilder;

const ITERATIONS: u32 = 10_000;
const WARMUP: u32 = 100;

fn bench<F: FnMut()>(name: &str, mut f: F) -> Duration {
    // Warmup
    for _ in 0..WARMUP {
        f();
    }

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        f();
    }
    let total = start.elapsed();
    let per_iter = total / ITERATIONS;

    println!("{name}: {per_iter:?}");
    per_iter
}

fn main() {
    println!("=== fuzzy-regex Benchmark ===\n");
    println!("Iterations: {ITERATIONS}\n");

    // Test 1: Basic search
    println!("--- Test 1: Basic Search ---");
    println!("Pattern: 'saddam' with 2 edits, Text: 70 chars");

    let text1 = "this is a saddamhu example with multiple saddam matches and ddamhu too";

    let fr1 = FuzzyRegexBuilder::new("(?:saddam)")
        .edits(2)
        .similarity(0.5)
        .build()
        .unwrap();

    bench("find", || {
        black_box(fr1.find(text1));
    });
    println!();

    // Test 2: Long text
    println!("--- Test 2: Long Text (519 chars) ---");
    let long_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut, commodo accumsan mi. Vestibulum porta, orci nec ullamcorper posuere, eros tortor pharetra est, at porttitor mi leo a velit. Aenean sollicitudin mauris elit, ultricies congue dui vulputate in. In hac habitasse platea dictumst. Nam iaculis sagittis justo a condimentum. Curabitur sed rhoncus dolor. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus egestas congue lorem, in convallis magna viverra quis.";
    println!("Pattern: 'tincidunt' with 1 edit, case-insensitive");

    let fr2 = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .case_insensitive(true)
        .similarity(0.8)
        .build()
        .unwrap();

    bench("find", || {
        black_box(fr2.find(long_text));
    });
    println!();

    // Test 3: Very long text
    println!("--- Test 3: Very Long Text (5190 chars) ---");
    let very_long_text = long_text.repeat(10);
    println!("Pattern: 'tincidunt' with 1 edit, case-insensitive");

    bench("find", || {
        black_box(fr2.find(&very_long_text));
    });
    println!();

    // Test 4: Short pattern
    println!("--- Test 4: Short Pattern ---");
    let text4 = "The quick brown fox jumps over the lazy dog";
    println!("Pattern: 'fox' with 1 edit, Text: {} chars", text4.len());

    let fr4 = FuzzyRegexBuilder::new("(?:fox)")
        .edits(1)
        .similarity(0.6)
        .build()
        .unwrap();

    bench("find", || {
        black_box(fr4.find(text4));
    });
    println!();

    // Test 5: High edit distance
    println!("--- Test 5: High Edit Distance (4 edits) ---");
    let text5 = "saddam hussein example text with multiple patterns";
    println!("Pattern: 'saddam' with 4 edits, Text: {} chars", text5.len());

    let fr5 = FuzzyRegexBuilder::new("(?:saddam)")
        .edits(4)
        .similarity(0.3)
        .build()
        .unwrap();

    bench("find", || {
        black_box(fr5.find(text5));
    });
    println!();

    // Test 6: DNA pattern
    println!("--- Test 6: DNA Pattern ---");
    let dna = "ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT";
    println!("Pattern: 'ACGTACGT' with 2 edits, Text: {} chars", dna.len());

    let fr6 = FuzzyRegexBuilder::new("(?:ACGTACGT)")
        .edits(2)
        .similarity(0.7)
        .build()
        .unwrap();

    bench("find", || {
        black_box(fr6.find(dna));
    });
    println!();

    // Test 7: Multiple patterns (alternation)
    println!("--- Test 7: Multiple Patterns (alternation) ---");
    let text7 = "cat bat rat cat mat sat cat pat fat hat";
    println!("Pattern: '(cat|bat|rat)' with 1 edit each");

    let fr7 = FuzzyRegexBuilder::new("(?:cat)|(?:bat)|(?:rat)")
        .edits(1)
        .similarity(0.6)
        .build()
        .unwrap();

    bench("find", || {
        black_box(fr7.find(text7));
    });
}
