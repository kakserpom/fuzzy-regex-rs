use fuzzy_regex::FuzzyRegex;
use regex::Regex;
use std::time::Instant;

fn main() {
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let long_text = short_text.repeat(100);

    let tests = [
        ("quick", "exact found"),
        ("nonexistent", "exact not found"),
        ("(?:quick|brown|fox)", "alternation"),
        ("quick.*fox", "wildcard"),
        (r"\b\w+\b", "word boundary"),
        (r"\d+", "digit"),
        (r"[a-z]+", "character class"),
        ("qu?ick", "optional"),
        ("(?:quick){2}", "repetition"),
    ];

    println!(
        "{:30} {:>15} {:>15} {:>15}",
        "", "fuzzy-rx short", "fuzzy-rx long", "regex long",
    );
    println!("{}", "-".repeat(75));

    for (pattern, desc) in tests {
        let fre = FuzzyRegex::new(pattern).unwrap();
        let rex = Regex::new(pattern).unwrap();

        // Warmup
        for _ in 0..100 {
            let _ = fre.find(short_text);
            let _ = rex.find(short_text);
        }

        // FuzzyRegex short
        let start = Instant::now();
        for _ in 0..10000 {
            let _ = fre.find(short_text);
        }
        let fuzzy_short = start.elapsed().as_nanos() as f64 / 10000.0;

        // FuzzyRegex long
        for _ in 0..10 {
            let _ = fre.find(&long_text);
        }
        let start = Instant::now();
        for _ in 0..100 {
            let _ = fre.find(&long_text);
        }
        let fuzzy_long = start.elapsed().as_nanos() as f64 / 100.0;

        // Regex long
        for _ in 0..10 {
            let _ = rex.find(&long_text);
        }
        let start = Instant::now();
        for _ in 0..100 {
            let _ = rex.find(&long_text);
        }
        let regex_long = start.elapsed().as_nanos() as f64 / 100.0;

        println!(
            "{:30} {:15.1} {:15.1} {:15.1}",
            desc, fuzzy_short, fuzzy_long, regex_long,
        );
    }
}
