//! Profile different layers of the code

use std::time::Instant;

use fuzzy_regex::FuzzyRegexBuilder;

fn bench<T, F: FnMut() -> T>(name: &str, iterations: u32, mut f: F) -> T {
    // Warmup
    for _ in 0..10 {
        f();
    }
    let start = Instant::now();
    let mut result = f();
    for _ in 1..iterations {
        result = f();
    }
    println!("{}: {:?} per iter", name, start.elapsed() / iterations);
    result
}

fn profile_with_greedy(text: &str, text_lower: &str) {
    let case_insensitive_regex = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .case_insensitive(true)
        .similarity(0.8)
        .build()
        .unwrap();

    let case_sensitive_regex = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .similarity(0.8)
        .build()
        .unwrap();

    // Profile full find() - this is what we're measuring
    println!("=== Full find() ===");
    let _ = bench("CI find()", 10_000, || case_insensitive_regex.find(text));
    let _ = bench("CS find()", 10_000, || case_sensitive_regex.find(text_lower));
}

fn profile_without_greedy(text: &str, text_lower: &str) {
    println!("\n=== With global mode ===");
    let case_insensitive_no_greedy = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .case_insensitive(true)
        .similarity(0.8)
        .build()
        .unwrap();

    let case_sensitive_no_greedy = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .similarity(0.8)
        .build()
        .unwrap();

    let _ = bench("CI find() no-greedy", 1_000, || {
        case_insensitive_no_greedy.find(text)
    });
    let _ = bench("CS find() no-greedy", 1_000, || {
        case_sensitive_no_greedy.find(text_lower)
    });
}

fn profile_is_match(text: &str, text_lower: &str) {
    let case_insensitive_regex = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .case_insensitive(true)
        .similarity(0.8)
        .build()
        .unwrap();

    let case_sensitive_regex = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .similarity(0.8)
        .build()
        .unwrap();

    // Compare with is_match
    println!("\n=== is_match() ===");
    let _ = bench("CI is_match()", 10_000, || {
        case_insensitive_regex.is_match(text)
    });
    let _ = bench("CS is_match()", 10_000, || {
        case_sensitive_regex.is_match(text_lower)
    });
}

fn main() {
    let text =
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut";
    let text_lower = text.to_lowercase();

    println!("Text length: {} bytes\n", text.len());

    profile_with_greedy(text, &text_lower);
    profile_without_greedy(text, &text_lower);
    profile_is_match(text, &text_lower);
}
