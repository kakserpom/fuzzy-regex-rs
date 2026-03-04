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
    println!("{name:45} {ns:>10.1} ns",);
}

fn main() {
    let text = "The quick brown fox jumps over the lazy dog 123.";

    println!("=== Slow Cases Analysis ===\n");

    // 1. No match - why is fuzzy 6.6x slower?
    println!("1. No match case:");
    let re_fuzzy = FuzzyRegex::new("nonexistent").unwrap();
    let re_regex = Regex::new("nonexistent").unwrap();

    bench("fuzzy (no match)", 50000, || {
        let _ = re_fuzzy.find(text);
    });
    bench("regex (no match)", 50000, || {
        let _ = re_regex.find(text);
    });

    // 2. End anchor - why 6.6x slower?
    println!("\n2. End anchor case:");
    let re_fuzzy = FuzzyRegex::new("dog$").unwrap();
    let re_regex = Regex::new("dog$").unwrap();

    bench("fuzzy (end anchor)", 50000, || {
        let _ = re_fuzzy.find(text);
    });
    bench("regex (end anchor)", 50000, || {
        let _ = re_regex.find(text);
    });

    // 3. Word with exact length - 163x slower!!!
    println!("\n3. Exact word length:");
    let re_fuzzy = FuzzyRegex::new(r"\b\w{4}\b").unwrap();
    let re_regex = Regex::new(r"\b\w{4}\b").unwrap();

    bench("fuzzy (4-char word)", 50000, || {
        let _ = re_fuzzy.find(text);
    });
    bench("regex (4-char word)", 50000, || {
        let _ = re_regex.find(text);
    });

    // 4. Lazy quantifier - 23x slower
    println!("\n4. Lazy quantifier:");
    let re_fuzzy = FuzzyRegex::new(r"\d+?").unwrap();
    let re_regex = Regex::new(r"\d+?").unwrap();

    bench("fuzzy (lazy digits)", 50000, || {
        let _ = re_fuzzy.find(text);
    });
    bench("regex (lazy digits)", 50000, || {
        let _ = re_regex.find(text);
    });

    // 5. Repetition - 6x slower
    println!("\n5. Repetition:");
    let re_fuzzy = FuzzyRegex::new(r"(?:quick){2}").unwrap();
    let re_regex = Regex::new(r"(?:quick){2}").unwrap();

    bench("fuzzy (repetition)", 50000, || {
        let _ = re_fuzzy.find(text);
    });
    bench("regex (repetition)", 50000, || {
        let _ = re_regex.find(text);
    });

    // Check what code path each pattern uses
    println!("\n=== Pattern Analysis ===");

    let patterns = [
        "nonexistent",
        "dog$",
        r"\b\w{4}\b",
        r"\d+?",
        r"(?:quick){2}",
    ];

    for p in patterns {
        let re = FuzzyRegex::new(p).unwrap();
        let literals = re.literals();
        println!("\nPattern: {}", p);
        println!("  literals: {}", literals.len());
    }
}
