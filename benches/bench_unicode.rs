//! Unicode mode benchmarks.
//! Run with: cargo bench --bench bench_unicode

use fuzzy_regex::FuzzyRegex;
use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

const SHORT_TEXT: &str = "The quick brown fox jumps over the lazy dog.";
const UNICODE_TEXT: &str = "Привет мир! Hello World. こんにちは世界 🔥";

fn bench<F>(name: &str, iterations: u32, mut f: F)
where
    F: FnMut(),
{
    // Warmup
    for _ in 0..10 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();

    let per_iter_us = elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations);

    println!("{name:45} {per_iter_us:>10.2} us/iter ({iterations} iters)");
}

fn main() {
    println!("fuzzy-regex Unicode Benchmarks");
    println!("===============================");
    println!();

    // Compilation benchmarks
    println!("--- Compilation ---");
    bench("compile ASCII pattern (no unicode)", 1000, || {
        let _ = FuzzyRegex::new("(?:hello){e<=2}").unwrap();
    });
    bench("compile ASCII pattern (unicode mode)", 1000, || {
        let _ = FuzzyRegexBuilder::new("(?:hello){e<=2}")
            .unicode(true)
            .build()
            .unwrap();
    });
    bench("compile Unicode pattern (unicode mode)", 1000, || {
        let _ = FuzzyRegexBuilder::new("(?:привет){e<=2}")
            .unicode(true)
            .build()
            .unwrap();
    });
    println!();

    // Short text benchmarks - ASCII
    println!("--- Short ASCII text ({} bytes) ---", SHORT_TEXT.len());
    let re_ascii_exact = FuzzyRegex::new("quick").unwrap();
    bench("ASCII exact match", 10000, || {
        let _ = re_ascii_exact.find(SHORT_TEXT);
    });

    let re_ascii_fuzzy = FuzzyRegex::new("(?:quikc){e<=1}").unwrap();
    bench("ASCII fuzzy 1 edit", 10000, || {
        let _ = re_ascii_fuzzy.find(SHORT_TEXT);
    });
    println!();

    // Short text benchmarks - Unicode
    println!("--- Short Unicode text ({} bytes) ---", UNICODE_TEXT.len());
    let re_uni_exact = FuzzyRegexBuilder::new("Hello")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode exact match", 10000, || {
        let _ = re_uni_exact.find(UNICODE_TEXT);
    });

    let re_uni_fuzzy = FuzzyRegexBuilder::new("Helo")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode fuzzy 1 edit", 10000, || {
        let _ = re_uni_fuzzy.find(UNICODE_TEXT);
    });

    let re_uni_sub = FuzzyRegexBuilder::new("Hallo")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode substitution", 10000, || {
        let _ = re_uni_sub.find(UNICODE_TEXT);
    });
    println!();

    // Word character class benchmarks
    println!("--- Unicode word class \\w ---");
    let re_w_ascii = FuzzyRegex::new(r"\w+").unwrap();
    bench("ASCII \\w+ (no unicode)", 10000, || {
        let _ = re_w_ascii.find("hello_world_123");
    });

    let re_w_uni = FuzzyRegexBuilder::new(r"\w+")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode \\w+ (unicode mode)", 10000, || {
        let _ = re_w_uni.find("hello_привет_123_日本語");
    });
    println!();

    // Digit character class benchmarks
    println!("--- Unicode digit class \\d ---");
    let re_d_ascii = FuzzyRegex::new(r"\d+").unwrap();
    bench("ASCII \\d+ (no unicode)", 10000, || {
        let _ = re_d_ascii.find("abc123def");
    });

    let re_d_uni = FuzzyRegexBuilder::new(r"\d+")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode \\d+ (unicode mode)", 10000, || {
        let _ = re_d_uni.find("abc١٢٣def"); // Arabic-Indic digits
    });
    println!();

    // Long text unicode benchmarks
    println!("--- Long text with Unicode ---");
    let long_text: String = UNICODE_TEXT.repeat(100);
    println!(
        "Text length: {} bytes ({} chars)",
        long_text.len(),
        long_text.chars().count()
    );

    let re_long = FuzzyRegexBuilder::new("(?:World){e<=1}")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode fuzzy find (long text)", 100, || {
        let _ = re_long.find(&long_text);
    });

    let re_long_iter = FuzzyRegexBuilder::new("(?:World){e<=1}")
        .unicode(true)
        .build()
        .unwrap();
    bench("Unicode fuzzy find_iter (long text)", 100, || {
        let _: usize = re_long_iter.find_iter(&long_text).count();
    });
    println!();

    // Comparison: ASCII vs Unicode mode on ASCII text
    println!("--- Mode comparison on ASCII text ---");
    let re_ascii = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    bench("ASCII pattern, no unicode flag", 10000, || {
        let _ = re_ascii.find("hello world");
    });

    let re_ascii_u = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .unicode(true)
        .build()
        .unwrap();
    bench("ASCII pattern, unicode flag", 10000, || {
        let _ = re_ascii_u.find("hello world");
    });
    println!();

    // Fuzzy unicode with Cyrillic
    println!("--- Cyrillic fuzzy matching ---");
    let cyrillic_text = "Это тестовая строка для проверки нечёткого поиска.";

    let re_cyrillic = FuzzyRegexBuilder::new("(?:строка){e<=1}")
        .unicode(true)
        .build()
        .unwrap();
    bench("Cyrillic fuzzy 1 edit", 10000, || {
        let _ = re_cyrillic.find(cyrillic_text);
    });

    let re_cyrillic_2 = FuzzyRegexBuilder::new("(?:строка){e<=2}")
        .unicode(true)
        .build()
        .unwrap();
    bench("Cyrillic fuzzy 2 edits", 10000, || {
        let _ = re_cyrillic_2.find(cyrillic_text);
    });
    println!();

    println!("Done!");
}
