use fuzzy_regex::FuzzyRegex;
use regex::Regex;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) {
    for _ in 0..100 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    let ns = start.elapsed().as_nanos() as f64 / f64::from(iters);
    println!("{name:35} {ns:>10.1} ns",);
}

fn main() {
    let short_text = "The quick brown fox jumps over the lazy dog 123.";
    let long_text = short_text.repeat(100);
    let very_long_text = short_text.repeat(1000);

    println!("=== fuzzy-regex vs regex crate Performance Comparison ===\n");

    println!("Text sizes:");
    println!("  short: {} bytes", short_text.len());
    println!("  long: {} bytes", long_text.len());
    println!("  very long: {} bytes\n", very_long_text.len());

    // Warmup all patterns
    println!("Warming up...");
    let warmup_patterns = [
        "quick",
        "nonexistent",
        "(?:quick|brown|fox)",
        "quick.*fox",
        r"\b\w+\b",
        r"\d+",
        r"[a-z]+",
        "qu?ick",
        "(?:quick){2}",
        "^The",
        "dog$",
        r"\b\w{4}\b",
        r"\d{3}",
        r"[a-z0-9]+",
        "quick.*brown.*fox",
        "(?:a|b|c|d|e)",
        "(?:quick){3}",
        "a*",
        "a+",
    ];
    for p in warmup_patterns {
        let fre = FuzzyRegex::new(p).unwrap();
        let rex = Regex::new(p).unwrap();
        for _ in 0..10 {
            let _ = fre.find(short_text);
            let _ = rex.find(short_text);
        }
    }

    println!("\n=== Short Text ({} bytes) ===\n", short_text.len());

    let tests: Vec<(&str, &str)> = vec![
        // Basic patterns
        ("quick", "exact literal"),
        ("nonexistent", "no match"),
        ("qu?ick", "optional char"),
        ("qu+ick", "one-or-more"),
        ("qu*ick", "zero-or-more"),
        // Anchors
        ("^The", "start anchor"),
        ("dog$", "end anchor"),
        ("^The quick", "anchored start"),
        // Character classes
        (r"[a-z]+", "lowercase class"),
        (r"[A-Z]+", "uppercase class"),
        (r"[0-9]+", "digit class"),
        (r"[a-zA-Z]+", "alpha class"),
        (r"[a-zA-Z0-9]+", "alphanumeric"),
        // Shorthand classes
        (r"\d+", "digits (\\d+)"),
        (r"\w+", "word chars (\\w+)"),
        (r"\s+", "whitespace (\\s+)"),
        (r"\D+", "non-digits"),
        (r"\W+", "non-word"),
        // Word boundaries
        (r"\b\w+\b", "word boundary"),
        (r"\b\w{4}\b", "4-char word"),
        (r"\b\w{3,6}\b", "3-6 char word"),
        // Quantifiers
        (r"\d{3}", "exactly 3 digits"),
        (r"\d{2,4}", "2-4 digits"),
        (r"\d+?", "lazy digits"),
        // Alternation
        ("(?:quick|brown|fox)", "alternation 3"),
        ("(?:a|b|c|d|e)", "alternation 5"),
        ("(?:one|two|three|four|five)", "alternation 5 words"),
        // Complex patterns
        ("quick.*fox", "wildcard"),
        ("quick.*brown.*fox", "multi wildcard"),
        ("(?:quick){2}", "repetition 2x"),
        ("(?:quick){3}", "repetition 3x"),
        // Mixed
        (r"\d+\.\d+", "decimal number"),
        (r"[a-z]+\d+", "letters then digits"),
        (r"\d+[a-z]+", "digits then letters"),
    ];

    for (pattern, desc) in &tests {
        let fre = FuzzyRegex::new(pattern).unwrap();

        bench(
            &format!("fuzzy {:20}", format!("({})", desc)),
            50000,
            || {
                let _ = fre.find(short_text);
            },
        );
    }

    println!();
    for (pattern, desc) in &tests {
        let rex = Regex::new(pattern).unwrap();
        bench(
            &format!("regex  {:20}", format!("({})", desc)),
            50000,
            || {
                let _ = rex.find(short_text);
            },
        );
    }

    println!("\n=== Long Text ({} bytes) ===\n", long_text.len());

    println!("Selected patterns (fuzzy):");
    let selected = [
        "quick",
        r"\d+",
        r"[a-z]+",
        "(?:quick){2}",
        "quick.*fox",
        r"\b\w+\b",
    ];
    for pattern in &selected {
        let fre = FuzzyRegex::new(pattern).unwrap();
        bench(
            &format!("fuzzy {:20}", format!("({})", pattern)),
            10000,
            || {
                let _ = fre.find(&long_text);
            },
        );
    }

    println!();
    println!("Selected patterns (regex):");
    for pattern in &selected {
        let rex = Regex::new(pattern).unwrap();
        bench(
            &format!("regex  {:20}", format!("({})", pattern)),
            10000,
            || {
                let _ = rex.find(&long_text);
            },
        );
    }

    println!(
        "\n=== Very Long Text ({} bytes) ===\n",
        very_long_text.len()
    );

    println!("Selected patterns (fuzzy):");
    for pattern in &selected {
        let fre = FuzzyRegex::new(pattern).unwrap();
        bench(
            &format!("fuzzy {:20}", format!("({})", pattern)),
            1000,
            || {
                let _ = fre.find(&very_long_text);
            },
        );
    }

    println!();
    println!("Selected patterns (regex):");
    for pattern in &selected {
        let rex = Regex::new(pattern).unwrap();
        bench(
            &format!("regex  {:20}", format!("({})", pattern)),
            1000,
            || {
                let _ = rex.find(&very_long_text);
            },
        );
    }

    println!("\n=== Summary ===\n");

    // Quick comparison
    let summary_tests = [
        ("quick", "exact"),
        ("\\d+", "digit"),
        ("[a-z]+", "char class"),
        ("(?:quick|brown|fox)", "alternation"),
        ("(?:quick){2}", "repetition"),
        ("\\b\\w+\\b", "word boundary"),
        ("quick.*fox", "wildcard"),
    ];

    println!(
        "{:30} {:>12} {:>12} {:>10}",
        "", "fuzzy ns", "regex ns", "ratio"
    );
    println!("{}", "-".repeat(65));

    for (pattern, desc) in summary_tests {
        let fre = FuzzyRegex::new(pattern).unwrap();
        let rex = Regex::new(pattern).unwrap();

        let start = Instant::now();
        for _ in 0..50000 {
            let _ = fre.find(short_text);
        }
        let fuzzy = start.elapsed().as_nanos() as f64 / 50000.0;

        let start = Instant::now();
        for _ in 0..50000 {
            let _ = rex.find(short_text);
        }
        let rx = start.elapsed().as_nanos() as f64 / 50000.0;

        let ratio = if rx > fuzzy { rx / fuzzy } else { fuzzy / rx };
        let winner = if fuzzy < rx { "fuzzy" } else { "regex" };

        println!(
            "{:30} {:12.1} {:12.1} {:6.1}x {}",
            desc, fuzzy, rx, ratio, winner
        );
    }
}
