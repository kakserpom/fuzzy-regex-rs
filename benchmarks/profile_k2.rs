//! Profile k=2 match at end case

use std::time::Instant;

use fuzzy_regex::FuzzyRegexBuilder;

fn main() {
    let text = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";
    println!("Text: '{}' (len={})", text, text.len());

    let fuzzy_regex = FuzzyRegexBuilder::new("(?:saddam)~2")
        .similarity(0.6)
        .build()
        .unwrap();

    // Verify match
    let m = fuzzy_regex.find(text);
    println!(
        "Match: {:?}",
        m.as_ref().map(|m| (m.start(), m.end(), m.as_str()))
    );

    // Detailed timing
    let iterations: u32 = 1000;

    // Warmup
    for _ in 0..100 {
        let _ = fuzzy_regex.find(text);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = fuzzy_regex.find(text);
    }
    let elapsed = start.elapsed();
    println!("\nTime per find: {:?}", elapsed / iterations);

    // Check if it's using streaming or position-by-position
    println!("\nPattern info:");
    println!("  is_simple_fuzzy: {}", fuzzy_regex.is_simple_fuzzy());
}
