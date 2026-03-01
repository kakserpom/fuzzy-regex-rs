//! Profile full `FuzzyRegex` path for Russian vs English

use std::time::Instant;

use fuzzy_regex::FuzzyRegexBuilder;

/// Calculate elapsed time in nanoseconds per iteration
fn elapsed_ns(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(iterations)
}

fn profile_short_text(
    regex_russian: &fuzzy_regex::FuzzyRegex,
    regex_english: &fuzzy_regex::FuzzyRegex,
    iterations: u32,
) {
    let text_russian = "Привет мир!";
    let text_english = "Hello world!";

    // Warmup
    for _ in 0..1000 {
        let _ = regex_russian.find(text_russian);
        let _ = regex_english.find(text_english);
    }

    // Profile Russian
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_russian.find(text_russian);
    }
    let russian_time = elapsed_ns(start, iterations);

    // Profile English
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = regex_english.find(text_english);
    }
    let english_time = elapsed_ns(start, iterations);

    println!("=== FuzzyRegex.find() Performance ===");
    println!("Russian: {:.1} ns ({:.2} us)", russian_time, russian_time / 1000.0);
    println!("English: {:.1} ns ({:.2} us)", english_time, english_time / 1000.0);
    println!("Ratio: {:.2}x", russian_time / english_time);

    // Check results
    println!("\n=== Results ===");
    println!("Russian match: {:?}", regex_russian.find(text_russian));
    println!("English match: {:?}", regex_english.find(text_english));

    // Also test is_simple_fuzzy status
    println!("\n=== Pattern Info ===");
    println!("Russian is_simple_fuzzy: {}", regex_russian.is_simple_fuzzy());
    println!("English is_simple_fuzzy: {}", regex_english.is_simple_fuzzy());
}

fn profile_long_text(
    regex_russian: &fuzzy_regex::FuzzyRegex,
    regex_english: &fuzzy_regex::FuzzyRegex,
) {
    println!("\n=== Scaling with Text Length ===");
    let long_text_russian = "Это тест. ".repeat(100) + "Привет мир!";
    let long_text_english = "This is a test. ".repeat(100) + "Hello world!";

    let long_iterations: u32 = 10_000;

    let start = Instant::now();
    for _ in 0..long_iterations {
        let _ = regex_russian.find(&long_text_russian);
    }
    let russian_long = elapsed_ns(start, long_iterations);

    let start = Instant::now();
    for _ in 0..long_iterations {
        let _ = regex_english.find(&long_text_english);
    }
    let english_long = elapsed_ns(start, long_iterations);

    println!(
        "Long text Russian: {:.1} ns ({:.2} us)",
        russian_long,
        russian_long / 1000.0
    );
    println!(
        "Long text English: {:.1} ns ({:.2} us)",
        english_long,
        english_long / 1000.0
    );
    println!("Long text ratio: {:.2}x", russian_long / english_long);
}

fn main() {
    let iterations = 100_000;

    // Build regexes
    let regex_russian = FuzzyRegexBuilder::new("(?:Привет){e<=1}")
        .build()
        .unwrap();

    let regex_english = FuzzyRegexBuilder::new("(?:Hello){e<=1}")
        .build()
        .unwrap();

    profile_short_text(&regex_russian, &regex_english, iterations);
    profile_long_text(&regex_russian, &regex_english);
}
