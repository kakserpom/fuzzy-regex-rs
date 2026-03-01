//! Profile detailed timing
use std::time::Instant;
use fuzzy_regex::FuzzyRegexBuilder;

fn bench<F: FnMut()>(name: &str, mut f: F, iterations: u32) {
    // Warmup
    for _ in 0..10 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    println!("{name}: {elapsed:?} per iter ({iterations} iters)");
}

fn main() {
    let short_text = "tincidutn eu metus";
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut";
    let medium_lower = medium_text.to_lowercase();

    println!("Short text: {} bytes", short_text.len());
    println!("Medium text: {} bytes", medium_text.len());
    println!();

    // Build patterns
    let case_insensitive_re = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .case_insensitive(true)
        .similarity(0.8)
        .build()
        .unwrap();

    let case_sensitive_re = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .similarity(0.8)
        .build()
        .unwrap();

    // Test short text (match at start)
    println!("=== Short text (match at pos 0) ===");
    bench("CI", || { let _ = case_insensitive_re.find(short_text); }, 100_000);
    bench("CS", || { let _ = case_sensitive_re.find(short_text); }, 100_000);

    // Test medium text (match at pos 80)
    println!("\n=== Medium text (match at pos 80) ===");
    bench("CI on mixed", || { let _ = case_insensitive_re.find(medium_text); }, 10000);
    bench("CS on mixed", || { let _ = case_sensitive_re.find(medium_text); }, 10000);
    bench("CS on lower", || { let _ = case_sensitive_re.find(&medium_lower); }, 10000);

    // Test without default (non-global) to see NFA overhead
    println!("\n=== With global mode ===");
    let case_insensitive_slow = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .case_insensitive(true)
        .similarity(0.8)
        .build()
        .unwrap();

    let case_sensitive_slow = FuzzyRegexBuilder::new("(?:tincidunt)")
        .edits(1)
        .similarity(0.8)
        .build()
        .unwrap();

    bench("CI slow", || { let _ = case_insensitive_slow.find(medium_text); }, 1000);
    bench("CS slow", || { let _ = case_sensitive_slow.find(&medium_lower); }, 1000);
}
