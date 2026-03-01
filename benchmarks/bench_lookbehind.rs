use fuzzy_regex::FuzzyRegex;
use std::time::{Duration, Instant};

fn bench<F>(name: &str, iterations: u32, mut func: F) -> Duration
where
    F: FnMut(),
{
    // Warmup
    for _ in 0..10 {
        func();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        func();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations;
    println!("{name:40} {per_iter:>10?} per iter ({iterations} iters)");
    per_iter
}

/// Benchmark fixed-length lookbehind
fn benchmark_fixed_lookbehind(
    short_text: &str,
    medium_text: &str,
    long_text: &str,
    very_long_text: &str,
) {
    println!("--- Fixed-length lookbehind: (?<=hello )world ---\n");
    let regex = FuzzyRegex::new(r"(?<=hello )world").unwrap();

    bench("Short text (11 bytes)", 10_000, || {
        std::hint::black_box(regex.find(short_text));
    });
    bench("Medium text (111 bytes)", 10_000, || {
        std::hint::black_box(regex.find(medium_text));
    });
    bench("Long text (1011 bytes)", 1000, || {
        std::hint::black_box(regex.find(long_text));
    });
    bench("Very long text (10011 bytes)", 100, || {
        std::hint::black_box(regex.find(very_long_text));
    });

    // Verify correctness
    assert!(regex.find(short_text).is_some());
    assert!(regex.find(medium_text).is_some());
    println!();
}

/// Benchmark variable-length lookbehind
fn benchmark_variable_lookbehind(medium_text: &str, long_text: &str) {
    println!("--- Variable-length lookbehind: (?<=hello|hi )world ---\n");
    let regex = FuzzyRegex::new(r"(?<=hello |hi )world").unwrap();

    bench("Short text - 'hello world'", 10_000, || {
        std::hint::black_box(regex.find("hello world"));
    });
    bench("Short text - 'hi world'", 10_000, || {
        std::hint::black_box(regex.find("hi world"));
    });
    bench("Medium text", 10_000, || {
        std::hint::black_box(regex.find(medium_text));
    });
    bench("Long text", 1000, || {
        std::hint::black_box(regex.find(long_text));
    });

    assert!(regex.find("hello world").is_some());
    assert!(regex.find("hi world").is_some());
    println!();
}

/// Benchmark negative lookbehind
fn benchmark_negative_lookbehind() {
    println!("--- Negative lookbehind: (?<!hello )world ---\n");
    let regex = FuzzyRegex::new(r"(?<!hello )world").unwrap();

    bench("'hi world' (should match)", 10_000, || {
        std::hint::black_box(regex.find("hi world"));
    });
    bench("'hello world' (should not match)", 10_000, || {
        std::hint::black_box(regex.find("hello world"));
    });
    bench("Long text with match", 1000, || {
        let text = format!("{}{}", "x".repeat(1000), "hi world");
        std::hint::black_box(regex.find(&text));
    });

    assert!(regex.find("hi world").is_some());
    assert!(regex.find("hello world").is_none());
    println!();
}

/// Benchmark fuzzy lookbehind
fn benchmark_fuzzy_lookbehind() {
    println!("--- Fuzzy lookbehind: (?<=(?:hello){{e<=1}})world ---\n");
    let regex = FuzzyRegex::new(r"(?<=(?:hello){e<=1})world").unwrap();

    bench("'helloworld' (exact)", 10_000, || {
        std::hint::black_box(regex.find("helloworld"));
    });
    bench("'helooworld' (1 edit)", 10_000, || {
        std::hint::black_box(regex.find("helooworld"));
    });
    bench("'heloworld' (1 deletion)", 10_000, || {
        std::hint::black_box(regex.find("heloworld"));
    });
    bench("Long text with fuzzy match", 1000, || {
        let text = format!("{}{}", "x".repeat(1000), "helooworld");
        std::hint::black_box(regex.find(&text));
    });

    assert!(regex.find("helloworld").is_some());
    assert!(regex.find("helooworld").is_some());
    println!();
}

/// Benchmark comparison with vs without lookbehind
fn benchmark_lookbehind_comparison(medium_text: &str) -> (Duration, Duration, Duration) {
    println!("--- Comparison: with vs without lookbehind ---\n");

    let regex_lookbehind = FuzzyRegex::new(r"(?<=hello )world").unwrap();
    let regex_no_lookbehind = FuzzyRegex::new(r"world").unwrap();
    let regex_full = FuzzyRegex::new(r"hello world").unwrap();

    let time_lookbehind = bench("With lookbehind: (?<=hello )world", 10_000, || {
        std::hint::black_box(regex_lookbehind.find(medium_text));
    });
    let time_no_lookbehind = bench("Without lookbehind: world", 10_000, || {
        std::hint::black_box(regex_no_lookbehind.find(medium_text));
    });
    let time_full = bench("Full pattern: hello world", 10_000, || {
        std::hint::black_box(regex_full.find(medium_text));
    });

    (time_lookbehind, time_no_lookbehind, time_full)
}

/// Print throughput summary
fn print_throughput_summary(
    short_text: &str,
    medium_text: &str,
    long_text: &str,
    very_long_text: &str,
) {
    println!("\n--- Throughput Summary ---\n");

    for (name, text) in [
        ("Short (11B)", short_text.to_string()),
        ("Medium (111B)", medium_text.to_string()),
        ("Long (1KB)", long_text.to_string()),
        ("VeryLong (10KB)", very_long_text.to_string()),
    ] {
        let regex = FuzzyRegex::new(r"(?<=hello )world").unwrap();
        let iters: u32 = if text.len() > 5000 { 100 } else { 1000 };
        let bytes = u32::try_from(text.len()).expect("text too large");

        let start = Instant::now();
        for _ in 0..iters {
            std::hint::black_box(regex.find(&text));
        }
        let elapsed = start.elapsed();
        let throughput = f64::from(bytes) * f64::from(iters) / elapsed.as_secs_f64() / 1_000_000.0;
        println!("{name:20} {throughput:>8.2} MB/s");
    }
}

fn main() {
    println!("=== Lookbehind Benchmark ===\n");

    // Test texts of various sizes
    let short_text = "hello world";
    let medium_text = format!("{}{}", "x".repeat(100), "hello world");
    let long_text = format!("{}{}", "x".repeat(1000), "hello world");
    let very_long_text = format!("{}{}", "x".repeat(10_000), "hello world");

    println!("Text sizes:");
    println!("  Short:     {:>6} bytes", short_text.len());
    println!("  Medium:    {:>6} bytes", medium_text.len());
    println!("  Long:      {:>6} bytes", long_text.len());
    println!("  Very Long: {:>6} bytes\n", very_long_text.len());

    // 1. Fixed-length lookbehind
    benchmark_fixed_lookbehind(short_text, &medium_text, &long_text, &very_long_text);

    // 2. Variable-length lookbehind
    benchmark_variable_lookbehind(&medium_text, &long_text);

    // 3. Negative lookbehind
    benchmark_negative_lookbehind();

    // 4. Fuzzy lookbehind
    benchmark_fuzzy_lookbehind();

    // 5. Compare with simple pattern (no lookbehind)
    let (time_lookbehind, time_no_lookbehind, time_full) =
        benchmark_lookbehind_comparison(&medium_text);

    println!(
        "\nLookbehind overhead: {:.1}x vs simple pattern",
        time_lookbehind.as_secs_f64() / time_no_lookbehind.as_secs_f64()
    );
    println!(
        "Lookbehind vs full match: {:.1}x",
        time_lookbehind.as_secs_f64() / time_full.as_secs_f64()
    );

    // 6. Throughput summary
    print_throughput_summary(short_text, &medium_text, &long_text, &very_long_text);

    println!("\n=== Benchmark Complete ===");
}
