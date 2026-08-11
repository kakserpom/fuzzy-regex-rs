//! Comprehensive benchmark suite for fuzzy-regex.
//! Run with: cargo bench --bench bench_suite
//!
//! Sections:
//!   1. vs `regex` crate — short (48 B), long (4800 B), very long (48000 B) texts
//!   2. Streaming throughput (`BitapMatcher::find_first_streaming`)
//!   3. Pathological pattern (`Dfa` find_all vs two_pass vs hardened)

use fuzzy_regex::engine::bitap::BitapMatcher;
use fuzzy_regex::engine::damlev::EditLimits;
use fuzzy_regex::engine::dfa::Dfa;
use fuzzy_regex::FuzzyRegex;
use regex::Regex;
use std::hint::black_box;
use std::time::Instant;

const SHORT_TEXT: &str = "The quick brown fox jumps over the lazy dog! 123"; // 48 bytes

/// Measure a closure's per-iteration cost in ns with automatic calibration.
fn bench_ns<R>(mut f: impl FnMut() -> R) -> f64 {
    for _ in 0..5 {
        black_box(f());
    }
    let mut iters: u64 = 1;
    for _ in 0..8 {
        let start = Instant::now();
        for _ in 0..iters {
            black_box(f());
        }
        let dt = start.elapsed().as_secs_f64().max(1e-12);
        if dt >= 0.02 {
            break;
        }
        let want = 0.05;
        iters = ((iters as f64 * want / dt).round() as u64).clamp(iters + 1, 10_000_000);
    }
    let start = Instant::now();
    for _ in 0..iters {
        black_box(f());
    }
    start.elapsed().as_secs_f64() * 1e9 / iters as f64
}

/// Measure per-iteration ns for fuzzy-regex and the `regex` crate with the same pattern.
fn bench_pair(label: &str, pattern: &str, text: &str) {
    let fuzzy = FuzzyRegex::new(pattern).unwrap();
    let std_re = Regex::new(pattern).unwrap();
    let f = bench_ns(|| fuzzy.is_match(text));
    let r = bench_ns(|| std_re.is_match(text));
    println!("  {:28} {:>12.1} ns {:>12.1} ns", label, f, r);
}

const SHORT_ROWS: &[(&str, &str)] = &[
    ("exact literal", "quick"),
    ("no match", "xyzzy"),
    ("optional char qu?ick", "qu?ick"),
    ("one-or-more qu+ick", "qu+ick"),
    ("zero-or-more qu*ick", "qu*ick"),
    ("start anchor ^The", "^The"),
    ("end anchor dog$", "dog$"),
    ("lowercase class [a-z]+", "[a-z]+"),
    ("digit class [0-9]+", "[0-9]+"),
    ("digits \\d+", "\\d+"),
    ("word chars \\w+", "\\w+"),
    ("whitespace \\s+", "\\s+"),
    ("non-digits \\D+", "\\D+"),
    ("word boundary \\b\\w+\\b", "\\b\\w+\\b"),
    ("4-char word \\b\\w{4}\\b", "\\b\\w{4}\\b"),
    ("exactly 3 digits \\d{3}", "\\d{3}"),
    ("lazy digits \\d+?", "\\d+?"),
    ("alternation (?:quick|brown|fox)", "(?:quick|brown|fox)"),
    ("wildcard quick.*fox", "quick.*fox"),
    ("repetition (?:quick){2}", "(?:quick){2}"),
    ("decimal \\d+\\.\\d+", "\\d+\\.\\d+"),
];

const LONG_ROWS: &[(&str, &str)] = &[
    ("exact literal", "quick"),
    ("digits \\d+", "\\d+"),
    ("char class [a-z]+", "[a-z]+"),
    ("repetition (?:quick){2}", "(?:quick){2}"),
    ("wildcard quick.*fox", "quick.*fox"),
    ("word boundary \\b\\w+\\b", "\\b\\w+\\b"),
];

fn bench_vs_regex() {
    println!("=== vs regex crate: short text ({} bytes) ===", SHORT_TEXT.len());
    println!("  {:28} {:>12} {:>12}", "pattern", "fuzzy-regex", "regex crate");
    for (label, pattern) in SHORT_ROWS {
        bench_pair(label, pattern, SHORT_TEXT);
    }

    let long_text: String = SHORT_TEXT.repeat(100);
    println!("\n=== vs regex crate: long text ({} bytes) ===", long_text.len());
    println!("  {:28} {:>12} {:>12}", "pattern", "fuzzy-regex", "regex crate");
    for (label, pattern) in LONG_ROWS {
        bench_pair(label, pattern, &long_text);
    }

    let very_long_text: String = SHORT_TEXT.repeat(1000);
    println!("\n=== vs regex crate: very long text ({} bytes) ===", very_long_text.len());
    println!("  {:28} {:>12} {:>12}", "pattern", "fuzzy-regex", "regex crate");
    for (label, pattern) in LONG_ROWS {
        bench_pair(label, pattern, &very_long_text);
    }
}

fn stream_throughput(text: &str, pattern: &str, k: u8, iterations: u32) -> f64 {
    let matcher = BitapMatcher::new(pattern, EditLimits::new(k), false).unwrap();
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let start = Instant::now();
        for _ in 0..iterations {
            black_box(matcher.find_first_streaming(text.as_bytes(), 0.0));
        }
        let dt = start.elapsed().as_secs_f64();
        best = best.min(text.len() as f64 * f64::from(iterations) / dt / 1_000_000.0);
    }
    best
}

fn streaming_text(target: usize, typo: &str) -> String {
    let base = "The quick brown fox jumps over the lazy dog! ";
    let mut text = String::new();
    while text.len() + base.len() + typo.len() <= target {
        text.push_str(base);
    }
    while text.len() + typo.len() < target {
        text.push('x');
    }
    text.push_str(typo);
    text
}

fn bench_streaming() {
    let pattern = "transportation";
    let typo = "transporattion";

    println!("=== streaming: throughput by text size (k=2, typo at end) ===");
    let cases: &[(&str, usize, u32)] = &[
        ("114 B", 114, 10_000),
        ("2 KB", 2_000, 2_000),
        ("20 KB", 20_000, 200),
        ("200 KB", 200_000, 20),
    ];
    for (label, size, iters) in cases {
        let text = streaming_text(*size, typo);
        let mbps = stream_throughput(&text, pattern, 2, *iters);
        println!("  {:>6} ({:>5} bytes): {:>8.1} MB/s", label, text.len(), mbps);
    }

    println!("\n=== streaming: throughput by k (20 KB text, typo at end) ===");
    let text = streaming_text(20_000, typo);
    for k in [1u8, 2, 3] {
        let mbps = stream_throughput(&text, pattern, k, 200);
        println!("  k = {k}: {:>8.1} MB/s", mbps);
    }

    println!("\n=== streaming: no match (138 KB text, no pattern present, k=2) ===");
    let no_match = "The quick brown fox jumps over the lazy dog! ".repeat(3150);
    let mbps = stream_throughput(&no_match, pattern, 2, 20);
    println!("  {:>6} ({:>5} bytes): {:>8.1} MB/s", "no-match", no_match.len(), mbps);
}

fn make_dfa(pattern: &str) -> Option<Dfa> {
    let ast = fuzzy_regex::parser::parse(pattern).unwrap();
    let hir = fuzzy_regex::ir::lower(&ast, 0);
    let (nfa, literals) = fuzzy_regex::compiler::build_nfa(&hir);
    let bridge = if literals.is_empty() {
        None
    } else {
        fuzzy_regex::engine::FuzzyBridge::new(&literals, None, None, false, false)
    };
    Dfa::from_nfa(&nfa, bridge.as_ref(), false, false, 1.0)
}

fn bench_pathological() {
    println!("=== pathological: `.*a|b` on text of 'b's (find_all) ===");
    println!("  single-shot timing, ms (lower is better)");
    for size in [1_000usize, 5_000, 10_000] {
        let text = "b".repeat(size);

        let mut t_naive = f64::INFINITY;
        let mut t_two_pass = f64::INFINITY;
        let mut t_hardened = f64::INFINITY;
        let mut n = 0;
        for _ in 0..2 {
            let mut naive = make_dfa(".*a|b").unwrap();
            let _ = naive.find_all(&text);
            let start = Instant::now();
            let matches = naive.find_all(&text);
            n = matches.len();
            t_naive = t_naive.min(start.elapsed().as_secs_f64() * 1000.0);

            let mut two_pass = make_dfa(".*a|b").unwrap();
            let _ = two_pass.find_all_two_pass(&text);
            let start = Instant::now();
            let n2 = two_pass.find_all_two_pass(&text).len();
            t_two_pass = t_two_pass.min(start.elapsed().as_secs_f64() * 1000.0);

            let mut hardened = make_dfa(".*a|b").unwrap();
            let _ = hardened.find_all_hardened(&text);
            let start = Instant::now();
            let n3 = hardened.find_all_hardened(&text).len();
            t_hardened = t_hardened.min(start.elapsed().as_secs_f64() * 1000.0);

            assert_eq!(n, n2);
            assert_eq!(n, n3);
        }
        println!(
            "  {:>6} bytes: find_all {:>8.1} ms, two_pass {:>8.1} ms, hardened {:>8.1} ms ({} matches)",
            size, t_naive, t_two_pass, t_hardened, n
        );
    }
}

fn main() {
    println!("fuzzy-regex benchmark suite");
    println!("===========================");

    bench_vs_regex();

    println!("\n");
    bench_streaming();

    println!("\n");
    bench_pathological();

    println!("\nDone!");
}
