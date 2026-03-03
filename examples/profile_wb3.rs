use fuzzy_regex::FuzzyRegex;

fn main() {
    let re = FuzzyRegex::new(r"\bquick\b").unwrap();

    // Try to access internal state - we can't directly, but we can check behavior
    let text = "The quick brown fox.";

    // Let's see if find() uses fast path or find_iter
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = re.find(text);
    }
    println!(
        "find(): {:.2} us",
        start.elapsed().as_micros() as f64 / 1000.0
    );

    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = re.find_iter(text).next();
    }
    println!(
        "find_iter().next(): {:.2} us",
        start.elapsed().as_micros() as f64 / 1000.0
    );
}
