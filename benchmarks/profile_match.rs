//! Profile basic match timing

use std::time::Instant;

use fuzzy_regex::FuzzyRegex;

/// Calculate elapsed time in microseconds
fn elapsed_us(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000.0
}

fn main() {
    let text = "Lorem ipsum dolor sit amet";
    let regex = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();

    // Single match timing
    let start = Instant::now();
    let m = regex.find(text);
    let elapsed = elapsed_us(start);

    println!("Text: '{}' ({} bytes)", text, text.len());
    println!("Pattern: Lorem with e<=2");
    if let Some(m) = m {
        println!("Found: '{}' at {}-{}", m.as_str(), m.start(), m.end());
    }
    println!("Time: {elapsed:.2} us");

    // Check what's happening internally by testing without fuzzy
    let regex_exact = FuzzyRegex::new("Lorem").unwrap();
    let start = Instant::now();
    let _ = regex_exact.find(text);
    let elapsed_exact = elapsed_us(start);
    println!("\nExact 'Lorem': {elapsed_exact:.2} us");
}
