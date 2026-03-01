#![allow(clippy::cast_precision_loss)]
use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) -> f64 {
    // Warmup
    for _ in 0..100 { f(); }
    
    let start = Instant::now();
    for _ in 0..iters { f(); }
    let elapsed = start.elapsed();
    let ns_per_iter = elapsed.as_nanos() as f64 / f64::from(iters);
    
    let time_str = if ns_per_iter < 1000.0 {
        format!("{ns_per_iter:.1} ns")
    } else {
        format!("{:.2} µs", ns_per_iter / 1000.0)
    };
    println!("{name}: {time_str}");
    ns_per_iter
}

fn main() {
    let short_text = "The quick brown fox jumps over the lazy dog.";
    
    println!("=== Short pattern fuzzy matching ===\n");
    
    // Case 1: Simple fuzzy e<=1 (mrab wins 1.7x)
    println!("Pattern: quikc (typo for quick) with e<=1");
    let re = FuzzyRegex::new("(?:quikc){e<=1}").unwrap();
    println!("  Literals: {:?}", re.literals().iter().map(|l| &l.text).collect::<Vec<_>>());
    println!("  is_simple_fuzzy: {}", re.is_simple_fuzzy());
    bench("  find", 1000, || { let _ = re.find(short_text); });
    println!("  Result: {:?}\n", re.find(short_text).map(|m| m.as_str()));
    
    // Case 2: Fuzzy e<=2 (mrab wins 1.9x)
    println!("Pattern: qwick (2 edits from quick) with e<=2");
    let re = FuzzyRegex::new("(?:qwick){e<=2}").unwrap();
    bench("  find", 1000, || { let _ = re.find(short_text); });
    println!("  Result: {:?}\n", re.find(short_text).map(|m| m.as_str()));
    
    // Case 3: Short pattern in long text (mrab wins 2.4x)
    let long_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
        nisi ut aliquip ex ea commodo consequat.".repeat(100);
    println!("Pattern: lorem with e<=1 in {} bytes", long_text.len());
    let re = FuzzyRegex::new("(?i)(?:lorem){e<=1}").unwrap();
    println!("  Literals: {:?}", re.literals().iter().map(|l| &l.text).collect::<Vec<_>>());
    bench("  find", 100, || { let _ = re.find(&long_text); });
    println!("  Result: {:?}\n", re.find(&long_text).map(|m| m.as_str()));
    
    // Case 4: DNA pattern (mrab wins 12x)
    let dna: String = (0..10000).map(|i| ['A', 'C', 'G', 'T'][i % 4]).collect();
    println!("Pattern: ACGTACGT with e<=2 in {} bytes DNA", dna.len());
    let re = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();
    println!("  Literals: {:?}", re.literals().iter().map(|l| &l.text).collect::<Vec<_>>());
    bench("  find", 100, || { let _ = re.find(&dna); });
    println!("  Result: {:?}\n", re.find(&dna).map(|m| m.as_str()));
    
    // Test what happens with exact match for comparison
    println!("=== Exact match comparison ===\n");
    println!("Pattern: quick (exact)");
    let re_exact = FuzzyRegex::new("quick").unwrap();
    bench("  find", 1000, || { let _ = re_exact.find(short_text); });
    
    println!("\nPattern: (?:quick){{e<=0}} (fuzzy syntax but 0 edits)");
    let re_zero = FuzzyRegex::new("(?:quick){e<=0}").unwrap();
    bench("  find", 1000, || { let _ = re_zero.find(short_text); });
}
