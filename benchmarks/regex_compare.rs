#![allow(clippy::cast_precision_loss, clippy::unreadable_literal)]
//! Compare fuzzy-regex with standard regex crate

use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) {
    // Warmup
    for _ in 0..100 { f(); }
    
    let start = Instant::now();
    for _ in 0..iters { f(); }
    let elapsed = start.elapsed();
    let per_iter = elapsed.as_nanos() as f64 / f64::from(iters);
    println!("{name:45} {per_iter:>10.2} ns/iter");
}

fn main() {
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let medium_text = short_text.repeat(10);
    let long_text = short_text.repeat(100);
    let very_long_text = short_text.repeat(1000);
    
    println!("=== fuzzy-regex vs regex crate Comparison ===\n");
    println!("Short text: {} bytes", short_text.len());
    println!("Medium text: {} bytes", medium_text.len());
    println!("Long text: {} bytes", long_text.len());
    println!("Very long text: {} bytes\n", very_long_text.len());
    
    // Exact matching comparison
    println!("--- Exact Match (find first) ---");
    
    let re_std = regex::Regex::new("quick").unwrap();
    let re_fuzzy = fuzzy_regex::FuzzyRegex::new("quick").unwrap();
    
    bench("regex::find (short)", 100000, || { re_std.find(short_text); });
    bench("fuzzy-regex::find exact (short)", 100000, || { re_fuzzy.find(short_text); });
    
    bench("regex::find (medium)", 100000, || { re_std.find(&medium_text); });
    bench("fuzzy-regex::find exact (medium)", 100000, || { re_fuzzy.find(&medium_text); });
    
    bench("regex::find (long)", 10000, || { re_std.find(&long_text); });
    bench("fuzzy-regex::find exact (long)", 10000, || { re_fuzzy.find(&long_text); });
    
    bench("regex::find (very long)", 1000, || { re_std.find(&very_long_text); });
    bench("fuzzy-regex::find exact (very long)", 1000, || { re_fuzzy.find(&very_long_text); });
    
    // Find all matches
    println!("\n--- Find All Matches ---");
    
    bench("regex::find_iter (long)", 10000, || { re_std.find_iter(&long_text).count(); });
    bench("fuzzy-regex::find_iter exact (long)", 10000, || { re_fuzzy.find_iter(&long_text).count(); });
    
    // No match case
    println!("\n--- No Match (full scan) ---");
    
    let re_std_no = regex::Regex::new("xyzzy").unwrap();
    let re_fuzzy_no = fuzzy_regex::FuzzyRegex::new("xyzzy").unwrap();
    
    bench("regex::find no match (short)", 100000, || { re_std_no.find(short_text); });
    bench("fuzzy-regex::find no match (short)", 100000, || { re_fuzzy_no.find(short_text); });
    
    bench("regex::find no match (long)", 10000, || { re_std_no.find(&long_text); });
    bench("fuzzy-regex::find no match (long)", 10000, || { re_fuzzy_no.find(&long_text); });
    
    // Fuzzy matching (fuzzy-regex only)
    println!("\n--- Fuzzy Matching (fuzzy-regex only) ---");
    
    let re_fuzzy_1 = fuzzy_regex::FuzzyRegexBuilder::new("(?:quick){e<=1}")
        .build().unwrap();
    let re_fuzzy_2 = fuzzy_regex::FuzzyRegexBuilder::new("(?:quick){e<=2}")
        .build().unwrap();
    
    bench("fuzzy-regex e<=1 (short)", 100000, || { re_fuzzy_1.find(short_text); });
    bench("fuzzy-regex e<=2 (short)", 100000, || { re_fuzzy_2.find(short_text); });
    bench("fuzzy-regex e<=1 (long)", 10000, || { re_fuzzy_1.find(&long_text); });
    bench("fuzzy-regex e<=2 (long)", 10000, || { re_fuzzy_2.find(&long_text); });
    
    // Complex patterns
    println!("\n--- Complex Patterns ---");
    
    let re_std_alt = regex::Regex::new("quick|brown|lazy").unwrap();
    let re_fuzzy_alt = fuzzy_regex::FuzzyRegex::new("quick|brown|lazy").unwrap();
    
    bench("regex alternation (short)", 100000, || { re_std_alt.find(short_text); });
    bench("fuzzy-regex alternation (short)", 100000, || { re_fuzzy_alt.find(short_text); });
    
    let re_std_class = regex::Regex::new(r"[a-z]+").unwrap();
    let re_fuzzy_class = fuzzy_regex::FuzzyRegex::new(r"[a-z]+").unwrap();
    
    bench("regex char class (short)", 100000, || { re_std_class.find(short_text); });
    bench("fuzzy-regex char class (short)", 100000, || { re_fuzzy_class.find(short_text); });
    
    // Anchored patterns
    println!("\n--- Anchored Patterns ---");
    
    let re_std_start = regex::Regex::new("^The").unwrap();
    let re_fuzzy_start = fuzzy_regex::FuzzyRegex::new("^The").unwrap();
    
    bench("regex ^anchor (short)", 100000, || { re_std_start.find(short_text); });
    bench("fuzzy-regex ^anchor (short)", 100000, || { re_fuzzy_start.find(short_text); });
    
    let re_std_end = regex::Regex::new(r"dog\.$").unwrap();
    let re_fuzzy_end = fuzzy_regex::FuzzyRegex::new(r"dog\.$").unwrap();
    
    bench("regex $anchor (short)", 100000, || { re_std_end.find(short_text); });
    bench("fuzzy-regex $anchor (short)", 100000, || { re_fuzzy_end.find(short_text); });
    
    // Compilation time
    println!("\n--- Compilation Time ---");
    
    bench("regex::new simple", 10000, || { let _ = regex::Regex::new("quick"); });
    bench("fuzzy-regex::new simple", 10000, || { let _ = fuzzy_regex::FuzzyRegex::new("quick"); });
    
    bench("regex::new complex", 10000, || { let _ = regex::Regex::new(r"[a-z]+\d{2,4}[A-Z]*"); });
    bench("fuzzy-regex::new complex", 10000, || { let _ = fuzzy_regex::FuzzyRegex::new(r"[a-z]+\d{2,4}[A-Z]*"); });
    
    println!("\nDone!");
}
