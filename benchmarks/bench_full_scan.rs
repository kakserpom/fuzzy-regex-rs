//! Full scan benchmark - match at end or no match.

use std::hint::black_box;
use std::time::Instant;
use fuzzy_regex::FuzzyRegexBuilder;

const ITERATIONS: u32 = 10_000;
const WARMUP: u32 = 100;

fn bench<F: FnMut()>(name: &str, mut f: F) {
    for _ in 0..WARMUP { f(); }
    let start = Instant::now();
    for _ in 0..ITERATIONS { f(); }
    let per_iter = start.elapsed() / ITERATIONS;
    println!("{name}: {per_iter:?}");
}

fn main() {
    println!("=== fuzzy-regex - Full Scan Benchmark ===\n");

    // Test: Match at END (forces full scan)
    println!("--- Test: k=4, match at end ---");
    let text = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";
    let fr = FuzzyRegexBuilder::new("(?:saddam)~4")
        .similarity(0.3)
        .build()
        .unwrap();
    let m = fr.find(text);
    println!("Text length: {}, Match at: {:?}", text.len(), m.as_ref().map(|m| (m.start(), m.end())));
    bench("find", || { black_box(fr.find(text)); });
    println!();

    // Test: k=2, match at end
    println!("--- Test: k=2, match at end ---");
    let text2 = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";
    let fr2 = FuzzyRegexBuilder::new("(?:saddam)~2")
        .similarity(0.6)
        .build()
        .unwrap();
    let m = fr2.find(text2);
    println!("Match at: {:?}", m.as_ref().map(|m| (m.start(), m.end())));
    bench("find", || { black_box(fr2.find(text2)); });
    println!();

    // Test: No match at all (worst case)
    println!("--- Test: k=2, NO match ---");
    let text3 = "xxxx xxxx xxxx xxxx xxxx xxxx yyyyyy";
    let m = fr2.find(text3);
    println!("Match: {m:?}");
    bench("find", || { black_box(fr2.find(text3)); });
}
