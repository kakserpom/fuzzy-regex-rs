#![allow(clippy::cast_precision_loss, clippy::unreadable_literal)]
use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) -> f64 {
    for _ in 0..100 { f(); }
    let start = Instant::now();
    for _ in 0..iters { f(); }
    let ns = start.elapsed().as_nanos() as f64 / f64::from(iters);
    println!("{name}: {ns:.0} ns");
    ns
}

fn main() {
    // Minimal text to isolate overhead
    let text = "ACGTACGT"; // Exact 8 bytes - exact match
    
    println!("=== Minimal text: '{text}' (8 bytes) ===\n");
    
    // Pre-compile all patterns
    let re_exact = FuzzyRegex::new("ACGTACGT").unwrap();
    let re_e0 = FuzzyRegex::new("(?:ACGTACGT){e<=0}").unwrap();
    let re_e1 = FuzzyRegex::new("(?:ACGTACGT){e<=1}").unwrap();
    let re_e2 = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();
    
    println!("Pattern info:");
    println!("  ACGTACGT - is_simple_fuzzy: {}", re_exact.is_simple_fuzzy());
    println!("  e<=0 - is_simple_fuzzy: {}", re_e0.is_simple_fuzzy());
    println!("  e<=1 - is_simple_fuzzy: {}", re_e1.is_simple_fuzzy());
    println!();
    
    let exact_ns = bench("exact match", 100000, || { let _ = re_exact.find(text); });
    let _e0_ns = bench("e<=0", 100000, || { let _ = re_e0.find(text); });
    let e1_ns = bench("e<=1", 100000, || { let _ = re_e1.find(text); });
    let e2_ns = bench("e<=2", 100000, || { let _ = re_e2.find(text); });
    
    println!("\nOverhead analysis:");
    println!("  e<=1 overhead: {:.0} ns (vs exact)", e1_ns - exact_ns);
    println!("  e<=2 overhead: {:.0} ns (vs exact)", e2_ns - exact_ns);
    println!("  e<=1/exact ratio: {:.1}x", e1_ns / exact_ns);
    println!("  e<=2/exact ratio: {:.1}x", e2_ns / exact_ns);
    
    // Check is_match (simpler path)
    println!("\n=== is_match comparison ===");
    bench("is_match exact", 100000, || { let _ = re_exact.is_match(text); });
    bench("is_match e<=0", 100000, || { let _ = re_e0.is_match(text); });
    bench("is_match e<=1", 100000, || { let _ = re_e1.is_match(text); });
}
