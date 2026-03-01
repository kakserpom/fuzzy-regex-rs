//! Quick benchmark comparing fuzzy-regex performance.
//! Run with: cargo run --release --example quick_bench

use fuzzy_regex::FuzzyRegex;
use std::time::{Duration, Instant};

const SHORT_TEXT: &str = "The quick brown fox jumps over the lazy dog.";
const MEDIUM_TEXT: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
    Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
    Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
    nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
    reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";

fn generate_long_text() -> String {
    MEDIUM_TEXT.repeat(100)
}

fn bench<F>(name: &str, iterations: u32, mut f: F)
where
    F: FnMut(),
{
    // Warmup
    for _ in 0..10 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();

    let per_iter = elapsed / iterations;
    let per_iter_us = per_iter.as_nanos() as f64 / 1000.0;

    println!(
        "{:40} {:>10.2} us/iter ({} iters)",
        name, per_iter_us, iterations
    );
}

fn main() {
    println!("fuzzy-regex Quick Benchmarks");
    println!("============================");
    println!();

    let long_text = generate_long_text();

    // Compilation benchmarks
    println!("--- Compilation ---");
    bench("compile simple pattern", 1000, || {
        let _ = FuzzyRegex::new("(?:hello){e<=2}").unwrap();
    });
    bench("compile complex pattern", 1000, || {
        let _ = FuzzyRegex::new("(?:hello){i<=1,d<=1,s<=2,1i+1d<3}").unwrap();
    });
    println!();

    // Short text benchmarks
    println!("--- Short text ({} bytes) ---", SHORT_TEXT.len());
    let re_exact = FuzzyRegex::new("quick").unwrap();
    bench("exact match", 10000, || {
        let _ = re_exact.find(SHORT_TEXT);
    });

    let re_fuzzy_1 = FuzzyRegex::new("(?:quikc){e<=1}").unwrap();
    bench("fuzzy 1 edit", 10000, || {
        let _ = re_fuzzy_1.find(SHORT_TEXT);
    });

    let re_fuzzy_2 = FuzzyRegex::new("(?:qwick){e<=2}").unwrap();
    bench("fuzzy 2 edits", 10000, || {
        let _ = re_fuzzy_2.find(SHORT_TEXT);
    });

    let re_sub = FuzzyRegex::new("(?:quack){s<=2}").unwrap();
    bench("substitution constraint", 10000, || {
        let _ = re_sub.find(SHORT_TEXT);
    });

    let re_cost = FuzzyRegex::new("(?:quikc){1i+1d<3}").unwrap();
    bench("cost constraint", 10000, || {
        let _ = re_cost.find(SHORT_TEXT);
    });
    println!();

    // Long text benchmarks
    println!("--- Long text ({} bytes) ---", long_text.len());
    let re_lorem = FuzzyRegex::new("(?:lorem){e<=2}").unwrap();
    bench("fuzzy 2 edits (find first)", 100, || {
        let _ = re_lorem.find(&long_text);
    });

    bench("fuzzy 2 edits (find_iter count)", 100, || {
        let _: usize = re_lorem.find_iter(&long_text).count();
    });

    let re_dolor = FuzzyRegex::new("(?:dolor){e<=1}").unwrap();
    bench("fuzzy 1 edit (find_iter count)", 100, || {
        let _: usize = re_dolor.find_iter(&long_text).count();
    });

    let re_no_match = FuzzyRegex::new("(?:xyzzy){e<=1}").unwrap();
    bench("no match (full scan)", 100, || {
        let _ = re_no_match.find(&long_text);
    });
    println!();

    // Edit distance scaling
    println!("--- Edit distance scaling (long text) ---");
    let re_0 = FuzzyRegex::new("(?:lorem){e<=0}").unwrap();
    let re_1 = FuzzyRegex::new("(?:lorem){e<=1}").unwrap();
    let re_2 = FuzzyRegex::new("(?:lorem){e<=2}").unwrap();
    let re_3 = FuzzyRegex::new("(?:lorem){e<=3}").unwrap();

    bench("0 edits (exact)", 100, || {
        let _ = re_0.find(&long_text);
    });
    bench("1 edit", 100, || {
        let _ = re_1.find(&long_text);
    });
    bench("2 edits", 100, || {
        let _ = re_2.find(&long_text);
    });
    bench("3 edits", 100, || {
        let _ = re_3.find(&long_text);
    });
    println!();

    // DNA sequence benchmark
    println!("--- DNA sequence (10000 bp) ---");
    let dna: String = (0..10000)
        .map(|i| match i % 4 {
            0 => 'A',
            1 => 'C',
            2 => 'G',
            _ => 'T',
        })
        .collect();

    let re_dna = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();
    bench("find motif", 100, || {
        let _ = re_dna.find(&dna);
    });
    println!();

    println!("Done!");
}
