use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let text = "The quick brown fox jumps over the lazy dog.";

    // Test word boundary patterns
    let patterns = [
        (r"\bquick\b", "word boundary"),
        (r"\bquick", "word start"),
        (r"quick\b", "word end"),
        (r"\w+", "word"),
        (r"\w+\b", "word + boundary"),
    ];

    for (pattern, desc) in patterns {
        let re = FuzzyRegex::new(pattern).unwrap();

        // Warmup
        for _ in 0..100 {
            let _ = re.find(text);
        }

        let start = Instant::now();
        for _ in 0..1000 {
            let _ = re.find(text);
        }
        let elapsed = start.elapsed().as_nanos() as f64 / 1000.0;

        println!("{:20} {:.1} µs", desc, elapsed);
    }
}
