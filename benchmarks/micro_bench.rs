#![allow(clippy::cast_precision_loss)]
use fuzzy_regex::FuzzyRegex;
use std::time::Instant;
use std::hint::black_box;

fn main() {
    let text = "ACGTACGT";
    
    // Pre-compile patterns
    let re_exact = FuzzyRegex::new("ACGTACGT").unwrap();
    let re_e1 = FuzzyRegex::new("(?:ACGTACGT){e<=1}").unwrap();
    
    println!("Benchmarking find() on '{text}'");
    println!("iterations per measurement: 100k\n");
    
    // Warmup
    for _ in 0..10000 {
        black_box(re_exact.find(black_box(text)));
        black_box(re_e1.find(black_box(text)));
    }
    
    // Measure exact
    let iters = 100_000u64;
    let start = Instant::now();
    for _ in 0..iters { black_box(re_exact.find(black_box(text))); }
    let exact_ns = start.elapsed().as_nanos() as f64 / iters as f64;
    
    // Measure fuzzy
    let start = Instant::now();
    for _ in 0..iters { black_box(re_e1.find(black_box(text))); }
    let fuzzy_ns = start.elapsed().as_nanos() as f64 / iters as f64;
    
    println!("exact:     {exact_ns:>7.1} ns");
    println!("e<=1:      {fuzzy_ns:>7.1} ns");
    println!("overhead:  {:>7.1} ns ({:.1}x)", fuzzy_ns - exact_ns, fuzzy_ns / exact_ns);
    
    // Now test is_simple_fuzzy path differences
    println!("\nPattern attributes:");
    println!("  exact is_simple_fuzzy: {}", re_exact.is_simple_fuzzy());
    println!("  e<=1 is_simple_fuzzy: {}", re_e1.is_simple_fuzzy());
    
    // Test directly calling find_iter (different path)
    println!("\nfind_iter().next() comparison:");
    let start = Instant::now();
    for _ in 0..iters { black_box(re_exact.find_iter(black_box(text)).next()); }
    let exact_iter_ns = start.elapsed().as_nanos() as f64 / iters as f64;
    
    let start = Instant::now();
    for _ in 0..iters { black_box(re_e1.find_iter(black_box(text)).next()); }
    let fuzzy_iter_ns = start.elapsed().as_nanos() as f64 / iters as f64;
    
    println!("exact:     {exact_iter_ns:>7.1} ns");
    println!("e<=1:      {fuzzy_iter_ns:>7.1} ns");
}
