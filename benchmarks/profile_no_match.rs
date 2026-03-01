#![allow(clippy::cast_precision_loss)]
use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
        nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
        reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";
    let long_text = medium_text.repeat(100);
    
    println!("Text length: {} bytes", long_text.len());
    
    // Pattern that won't match
    let patterns = [
        ("(?:xyzzy){e<=1}", "fuzzy no-match"),
        ("xyzzy", "exact no-match"),
        ("(?:lorem){e<=1}", "fuzzy match"),  
        ("lorem", "exact match"),
    ];
    
    for (pat, desc) in patterns {
        let re = FuzzyRegex::new(pat).unwrap();
        println!("\nPattern: {pat} ({desc})");
        println!("  literals: {:?}", re.literals().iter().map(|l| &l.text).collect::<Vec<_>>());
        println!("  is_simple_fuzzy: {}", re.is_simple_fuzzy());
        
        // Warmup
        for _ in 0..3 { let _ = re.find(&long_text); }
        
        let start = Instant::now();
        let iters = 10;
        for _ in 0..iters {
            let _ = re.find(&long_text);
        }
        let elapsed = start.elapsed();
        let per_iter = elapsed.as_micros() as f64 / f64::from(iters);
        println!("  time: {per_iter:.1} µs/iter");
        
        if let Some(m) = re.find(&long_text) {
            println!("  found: {:?}", m.as_str());
        } else {
            println!("  found: None");
        }
    }
}
