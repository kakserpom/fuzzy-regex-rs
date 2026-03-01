use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    println!("=== Simple Lookbehind Benchmark ===\n");

    // Create texts of different sizes
    for size in [10, 100, 500, 1000, 2000] {
        let text = format!("{}hello world", "x".repeat(size));

        let regex = FuzzyRegex::new(r"(?<=hello )world").unwrap();

        // Warmup
        for _ in 0..5 {
            regex.find(&text);
        }

        // Benchmark
        let iterations = 100u32;
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(regex.find(&text));
        }
        let elapsed = start.elapsed();
        let per_iter_us = elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations);

        println!("Size {:>5} bytes: {:>8.0} us/iter", text.len(), per_iter_us);
    }

    println!();

    // Compare: with vs without lookbehind
    let text = format!("{}hello world", "x".repeat(500));

    let regex_lookbehind = FuzzyRegex::new(r"(?<=hello )world").unwrap();
    let regex_no_lookbehind = FuzzyRegex::new(r"world").unwrap();
    let regex_full = FuzzyRegex::new(r"hello world").unwrap();

    let iterations = 100u32;

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(regex_lookbehind.find(&text));
    }
    let lookbehind_us = start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(regex_no_lookbehind.find(&text));
    }
    let no_lookbehind_us = start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(regex_full.find(&text));
    }
    let full_us = start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations);

    println!("Comparison on {} byte text:", text.len());
    println!("  With lookbehind:    {lookbehind_us:>8.0} us");
    println!("  Without lookbehind: {no_lookbehind_us:>8.0} us");
    println!("  Full pattern:       {full_us:>8.0} us");
    println!("  Lookbehind overhead: {:.1}x vs full", lookbehind_us / full_us);
}
