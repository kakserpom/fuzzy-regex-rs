use fuzzy_regex::FuzzyRegexBuilder;
use memchr::memmem;
use std::time::Instant;

/// Convert elapsed time to microseconds using f64
fn elapsed_us(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations)
}

fn test_minimal_match(iterations: u32) {
    println!("=== Match at start (minimal text) ===");

    let regex_russian = FuzzyRegexBuilder::new("(?:Привет){e<=1}")
        .build()
        .unwrap();
    let regex_english = FuzzyRegexBuilder::new("(?:Hello){e<=1}")
        .build()
        .unwrap();

    let text_russian = "Привет";
    let text_english = "Hello";

    // Warmup
    for _ in 0..1000 {
        let _ = regex_russian.find(text_russian);
        let _ = regex_english.find(text_english);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_russian.find(text_russian);
    }
    let russian_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_english.find(text_english);
    }
    let english_time = elapsed_us(start, iterations);

    println!("Russian: {russian_time:.2} µs");
    println!("English: {english_time:.2} µs");
    println!("Ratio: {:.2}x\n", russian_time / english_time);
}

fn test_prefilter_cost(iterations: u32) {
    println!("=== Testing prefilter cost ===");

    let regex_russian_exact = FuzzyRegexBuilder::new("(?:Привет){e<=0}")
        .build()
        .unwrap();
    let regex_english_exact = FuzzyRegexBuilder::new("(?:Hello){e<=0}")
        .build()
        .unwrap();

    let text_russian = "Привет";
    let text_english = "Hello";

    // Warmup
    for _ in 0..1000 {
        let _ = regex_russian_exact.find(text_russian);
        let _ = regex_english_exact.find(text_english);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_russian_exact.find(text_russian);
    }
    let russian_exact_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_english_exact.find(text_english);
    }
    let english_exact_time = elapsed_us(start, iterations);

    println!("Russian exact: {russian_exact_time:.2} µs");
    println!("English exact: {english_exact_time:.2} µs");
    println!("Ratio: {:.2}x\n", russian_exact_time / english_exact_time);
}

fn test_longer_patterns(iterations: u32) {
    println!("=== Longer patterns ===");

    let regex_russian_long = FuzzyRegexBuilder::new("(?:Привет){e<=1}")
        .build()
        .unwrap();
    let regex_english_long = FuzzyRegexBuilder::new("(?:Hellox){e<=1}")
        .build()
        .unwrap();

    let text_russian_long = "Привет";
    let text_english_long = "Hellox";

    for _ in 0..1000 {
        let _ = regex_russian_long.find(text_russian_long);
        let _ = regex_english_long.find(text_english_long);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_russian_long.find(text_russian_long);
    }
    let russian_long_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_english_long.find(text_english_long);
    }
    let english_long_time = elapsed_us(start, iterations);

    println!("Russian 6 chars: {russian_long_time:.2} µs");
    println!("English 6 chars: {english_long_time:.2} µs");
    println!("Ratio: {:.2}x\n", russian_long_time / english_long_time);
}

fn test_memmem_prefilter(iterations: u32) {
    println!("=== memmem prefilter test ===");

    let needle_russian = "Привет".as_bytes();
    let text_russian_bytes = "Привет мир!".as_bytes();
    let needle_english = b"Hello";
    let text_english_bytes = b"Hello world!";

    let finder_russian = memmem::Finder::new(needle_russian);
    let finder_english = memmem::Finder::new(needle_english);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = finder_russian.find(text_russian_bytes);
    }
    let russian_memmem_time = elapsed_us(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = finder_english.find(text_english_bytes);
    }
    let english_memmem_time = elapsed_us(start, iterations);

    println!("Russian memmem: {russian_memmem_time:.3} µs");
    println!("English memmem: {english_memmem_time:.3} µs");
    println!("Ratio: {:.2}x", russian_memmem_time / english_memmem_time);
}

fn main() {
    println!("=== Russian Performance Profile ===\n");

    let iterations: u32 = 100_000;

    test_minimal_match(iterations);
    test_prefilter_cost(iterations);

    println!("=== Timing breakdown: Pattern chars ===");
    println!("'Привет' = 6 Cyrillic chars = 12 UTF-8 bytes");
    println!("'Hello' = 5 ASCII chars = 5 bytes\n");

    test_longer_patterns(iterations);
    test_memmem_prefilter(iterations);
}
