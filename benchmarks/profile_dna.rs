#![allow(clippy::cast_precision_loss)]
use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let dna: String = (0..10000).map(|i| ['A', 'C', 'G', 'T'][i % 4]).collect();
    
    println!("DNA text: {} bytes", dna.len());
    println!("First 100 chars: {}\n", &dna[..100]);
    
    // Count each letter
    let a_count = dna.bytes().filter(|&b| b == b'A').count();
    let c_count = dna.bytes().filter(|&b| b == b'C').count();
    let g_count = dna.bytes().filter(|&b| b == b'G').count();
    let t_count = dna.bytes().filter(|&b| b == b'T').count();
    println!("Letter counts: A={a_count}, C={c_count}, G={g_count}, T={t_count}");
    println!("Each letter appears ~25% of the time\n");
    
    // Test different patterns
    let patterns = [
        ("(?:ACGTACGT){e<=0}", "exact"),
        ("(?:ACGTACGT){e<=1}", "e<=1"),
        ("(?:ACGTACGT){e<=2}", "e<=2"),
        ("ACGTACGT", "literal"),
    ];
    
    for (pat, desc) in patterns {
        let re = FuzzyRegex::new(pat).unwrap();
        println!("Pattern: {pat} ({desc})");
        println!("  Literals: {:?}", re.literals().iter().map(|l| &l.text).collect::<Vec<_>>());
        println!("  is_simple_fuzzy: {}", re.is_simple_fuzzy());
        
        // Warmup
        for _ in 0..10 { let _ = re.find(&dna); }
        
        // Time
        let start = Instant::now();
        let iters = 100;
        for _ in 0..iters { let _ = re.find(&dna); }
        let elapsed = start.elapsed();
        let ns_per = elapsed.as_nanos() as f64 / f64::from(iters);
        
        println!("  Time: {:.2} µs", ns_per / 1000.0);
        if let Some(m) = re.find(&dna) {
            println!("  Match: {:?} at {}..{}", m.as_str(), m.start(), m.end());
        }
        println!();
    }
    
    // The key question: how many prefilter candidates exist?
    // With pattern "ACGTACGT" and e<=2, prefilter searches for first 3 chars
    // That's A, C, G (from positions 0, 1, 2 of pattern)
    // In DNA, EVERY position matches one of these!
    println!("=== Prefilter analysis ===");
    println!("For e<=2, prefilter depth = min(3, 8) = 3");
    println!("First 3 chars of 'ACGTACGT' are A, C, G");
    println!("In DNA text, positions with A, C, or G: {}", 
        dna.bytes().filter(|&b| b == b'A' || b == b'C' || b == b'G').count());
    println!("That's 75% of all positions = ~7500 candidates!");
    println!("\nFor each candidate, we try Bitap which is fast but 7500 * ~0.6µs = ~4.5ms");
}
