//! Comprehensive benchmark matching mrab-regex test cases
#![allow(clippy::cast_precision_loss)]

use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

const SHORT_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

const MEDIUM_TEXT: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";

fn bench<F: FnMut()>(name: &str, iters: u32, warmup: u32, mut f: F) {
    for _ in 0..warmup {
        f();
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    let elapsed = start.elapsed();
    let mean_ns = elapsed.as_nanos() as f64 / f64::from(iters);

    let time_str = if mean_ns < 1000.0 {
        format!("{mean_ns:.1} ns")
    } else if mean_ns < 1_000_000.0 {
        format!("{:.2} us", mean_ns / 1000.0)
    } else {
        format!("{:.2} ms", mean_ns / 1_000_000.0)
    };

    println!("{name}:");
    println!("    mean:   {time_str}");
}

fn main() {
    let long_text = MEDIUM_TEXT.repeat(100);
    let very_long_text = MEDIUM_TEXT.repeat(1000);

    println!("{}", "=".repeat(60));
    println!("fuzzy-regex Benchmarks");
    println!("{}", "=".repeat(60));
    println!();

    // Exact match
    println!("exact_match_short:");
    let re = FuzzyRegex::new("quick").unwrap();
    bench("  search", 1000, 100, || { let _ = re.find(SHORT_TEXT); });
    println!();

    // Fuzzy match with 1 edit
    println!("fuzzy_1_edit_short:");
    let re = FuzzyRegex::new("(?:quikc){e<=1}").unwrap();
    bench("  search", 1000, 100, || { let _ = re.find(SHORT_TEXT); });
    println!();

    // Fuzzy match with 2 edits
    println!("fuzzy_2_edits_short:");
    let re = FuzzyRegex::new("(?:qwick){e<=2}").unwrap();
    bench("  search", 1000, 100, || { let _ = re.find(SHORT_TEXT); });
    println!();

    // Fuzzy match with substitution constraint
    println!("fuzzy_substitution_short:");
    let re = FuzzyRegex::new("(?:quack){s<=2}").unwrap();
    bench("  search", 1000, 100, || { let _ = re.find(SHORT_TEXT); });
    println!();

    // Fuzzy match with cost constraint
    println!("fuzzy_cost_constraint_short:");
    let re = FuzzyRegex::new("(?:quikc){1i+1d<3}").unwrap();
    bench("  search", 1000, 100, || { let _ = re.find(SHORT_TEXT); });
    println!();

    // Text size scaling (case-insensitive)
    println!("text_size_scaling:");
    let re = FuzzyRegex::new("(?i)(?:lorem){e<=2}").unwrap();
    bench("  medium_text", 500, 100, || { let _ = re.find(MEDIUM_TEXT); });
    bench("  long_text", 100, 10, || { let _ = re.find(&long_text); });
    bench("  very_long_text", 10, 2, || { let _ = re.find(&very_long_text); });
    println!();

    // Pattern length scaling (case-insensitive)
    println!("pattern_length_scaling:");
    let re_short = FuzzyRegex::new("(?i)(?:lorem){e<=1}").unwrap();
    let re_medium = FuzzyRegex::new("(?i)(?:consectetur){e<=2}").unwrap();
    let re_long = FuzzyRegex::new("(?i)(?:exercitation){e<=2}").unwrap();
    bench("  pattern_5_chars", 100, 10, || { let _ = re_short.find(&long_text); });
    bench("  pattern_11_chars", 100, 10, || { let _ = re_medium.find(&long_text); });
    bench("  pattern_13_chars", 100, 10, || { let _ = re_long.find(&long_text); });
    println!();

    // Edit distance scaling (case-insensitive)
    println!("edit_distance_scaling:");
    let re_0 = FuzzyRegex::new("(?i)(?:lorem){e<=0}").unwrap();
    let re_1 = FuzzyRegex::new("(?i)(?:lorem){e<=1}").unwrap();
    let re_2 = FuzzyRegex::new("(?i)(?:lorem){e<=2}").unwrap();
    let re_3 = FuzzyRegex::new("(?i)(?:lorem){e<=3}").unwrap();
    bench("  0_edits", 100, 10, || { let _ = re_0.find(&long_text); });
    bench("  1_edit", 100, 10, || { let _ = re_1.find(&long_text); });
    bench("  2_edits", 100, 10, || { let _ = re_2.find(&long_text); });
    bench("  3_edits", 100, 10, || { let _ = re_3.find(&long_text); });
    println!();

    // find_iter (case-insensitive)
    println!("find_iter:");
    let re = FuzzyRegex::new("(?i)(?:dolor){e<=1}").unwrap();
    bench("  find_iter_long_text", 100, 10, || { let _ = re.find_iter(&long_text).count(); });
    println!();

    // is_match (case-insensitive for found)
    println!("is_match:");
    let re_found = FuzzyRegex::new("(?i)(?:lorem){e<=2}").unwrap();
    let re_not_found = FuzzyRegex::new("(?:xyzzy){e<=1}").unwrap();
    bench("  is_match_found", 100, 10, || { let _ = re_found.is_match(&long_text); });
    bench("  is_match_not_found", 100, 10, || { let _ = re_not_found.is_match(&long_text); });
    println!();

    // Compilation
    println!("compilation:");
    bench("  simple_pattern", 1000, 100, || { let _ = FuzzyRegex::new("(?:hello){e<=2}"); });
    bench("  complex_pattern", 1000, 100, || { let _ = FuzzyRegex::new("(?:hello){i<=1,d<=1,s<=2,1i+1d<3}"); });
    println!();

    // Typo correction
    println!("typo_correction:");
    let document = "The recieve function should recieve data from the server. \
Make sure to recieve all packets before processing. \
If you don't recieve a response within 5 seconds, retry.";
    let re = FuzzyRegex::new("(?:receive){e<=2}").unwrap();
    bench("  find_misspellings", 1000, 100, || { let _ = re.find_iter(document).count(); });
    println!();

    // DNA sequence matching
    println!("dna_matching:");
    let dna: String = (0..10000).map(|i| ['A', 'C', 'G', 'T'][i % 4]).collect();
    let re = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();
    bench("  find_motif", 100, 10, || { let _ = re.find(&dna); });
    println!();

    println!("{}", "=".repeat(60));
    println!("Benchmarks complete!");
    println!("{}", "=".repeat(60));
}
