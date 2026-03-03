use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let long_text = short_text.repeat(100);

    println!("=== General Regex Performance ===\n");

    let tests = [
        // Exact patterns
        ("quick", "exact found"),
        ("nonexistent", "exact not found"),
        ("(?:quick|brown|fox)", "alternation"),
        ("quick.*fox", "wildcard"),
        (r"\b\w+\b", "word boundary"),
        (r"\d+", "digit"),
        (r"[a-z]+", "character class"),
        (r"qu?ick", "optional"),
        (r"(?:quick){2}", "repetition"),
        // Fuzzy patterns
        ("(?:quikc){e<=1}", "fuzzy 1 edit"),
        ("(?:qwick){e<=2}", "fuzzy 2 edits"),
    ];

    for (pattern, desc) in tests {
        let re = FuzzyRegex::new(pattern).unwrap();

        // Warmup
        for _ in 0..100 {
            let _ = re.find(short_text);
        }

        let start = Instant::now();
        for _ in 0..10000 {
            let _ = re.find(short_text);
        }
        let elapsed = start.elapsed().as_nanos() as f64 / 10000.0;

        // Also test long text
        for _ in 0..10 {
            let _ = re.find(&long_text);
        }
        let start = Instant::now();
        for _ in 0..100 {
            let _ = re.find(&long_text);
        }
        let elapsed_long = start.elapsed().as_nanos() as f64 / 100.0;

        println!(
            "{:25} short: {:7.1}ns  long: {:8.1}ns  ({})",
            desc,
            elapsed,
            elapsed_long,
            if elapsed > 1000.0 { "SLOW" } else { "" }
        );
    }
}
