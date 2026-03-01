//! Compare `fuzzy_regex::compat` with fuzzy-aho-corasick
//!
//! Run with: cargo run --release --example `compare_fac`

use std::hint::black_box;
use std::time::{Duration, Instant};

use fuzzy_regex::compat::{FuzzyAhoCorasickBuilder, FuzzyLimits};

const ITERATIONS: u32 = 1_000;
const WARMUP: u32 = 10;

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
    println!("=== fuzzy-regex::compat Benchmark ===\n");
    println!("Iterations: {ITERATIONS}\n");

    // Test 1: Basic search
    println!("--- Test 1: Basic Search ---");
    println!("Pattern: 'saddam' with 2 edits, Text: 70 chars");

    let text1 = "this is a saddamhu example with multiple saddam matches and ddamhu too";

    let fac1 = FuzzyAhoCorasickBuilder::new()
        .fuzzy(FuzzyLimits::new().edits(2))
        .build(["saddam"]);

    bench("search", || {
        black_box(fac1.search_non_overlapping(text1, 0.5));
    });
    println!();

    // Test 2: Long text
    println!("--- Test 2: Long Text (519 chars) ---");
    let long_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut, commodo accumsan mi. Vestibulum porta, orci nec ullamcorper posuere, eros tortor pharetra est, at porttitor mi leo a velit. Aenean sollicitudin mauris elit, ultricies congue dui vulputate in. In hac habitasse platea dictumst. Nam iaculis sagittis justo a condimentum. Curabitur sed rhoncus dolor. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus egestas congue lorem, in convallis magna viverra quis.";
    println!("Pattern: 'tincidunt' with 1 edit, case-insensitive");

    let fac2 = FuzzyAhoCorasickBuilder::new()
        .fuzzy(FuzzyLimits::new().edits(1))
        .case_insensitive(true)
        .build(["tincidunt"]);

    bench("search", || {
        black_box(fac2.search_non_overlapping(long_text, 0.8));
    });
    println!();

    // Test 3: Very long text
    println!("--- Test 3: Very Long Text (5190 chars) ---");
    let very_long_text = long_text.repeat(10);
    println!("Pattern: 'tincidunt' with 1 edit, case-insensitive");

    bench("search", || {
        black_box(fac2.search_non_overlapping(&very_long_text, 0.8));
    });
    println!();

    // Test 4: Short pattern
    println!("--- Test 4: Short Pattern ---");
    let text4 = "The quick brown fox jumps over the lazy dog";
    println!("Pattern: 'fox' with 1 edit, Text: {} chars", text4.len());

    let fac4 = FuzzyAhoCorasickBuilder::new()
        .fuzzy(FuzzyLimits::new().edits(1))
        .build(["fox"]);

    bench("search", || {
        black_box(fac4.search_non_overlapping(text4, 0.6));
    });
    println!();

    // Test 5: High edit distance
    println!("--- Test 5: High Edit Distance (4 edits) ---");
    let text5 = "saddam hussein example text with multiple patterns";
    println!("Pattern: 'saddam' with 4 edits, Text: {} chars", text5.len());

    let fac5 = FuzzyAhoCorasickBuilder::new()
        .fuzzy(FuzzyLimits::new().edits(4))
        .build(["saddam"]);

    bench("search", || {
        black_box(fac5.search_non_overlapping(text5, 0.3));
    });
    println!();

    // Test 6: DNA pattern
    println!("--- Test 6: DNA Pattern ---");
    let dna = "ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT";
    println!("Pattern: 'ACGTACGT' with 2 edits, Text: {} chars", dna.len());

    let fac6 = FuzzyAhoCorasickBuilder::new()
        .fuzzy(FuzzyLimits::new().edits(2))
        .build(["ACGTACGT"]);

    bench("search", || {
        black_box(fac6.search_non_overlapping(dna, 0.7));
    });
    println!();

    // Test 7: Multiple patterns
    println!("--- Test 7: Multiple Patterns ---");
    let text7 = "cat bat rat cat mat sat cat pat fat hat";
    println!("Patterns: ['cat', 'bat', 'rat'] with 1 edit each");

    let fac7 = FuzzyAhoCorasickBuilder::new()
        .fuzzy(FuzzyLimits::new().edits(1))
        .build(["cat", "bat", "rat"]);

    bench("search", || {
        black_box(fac7.search_non_overlapping(text7, 0.6));
    });
}
