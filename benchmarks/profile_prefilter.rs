//! Profile prefilter performance for Russian vs English

use std::time::Instant;

use fuzzy_regex::engine::prefilter::Prefilter;

/// Calculate elapsed time in nanoseconds per iteration
fn elapsed_ns(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(iterations)
}

fn profile_short_text(iterations: u32) {
    // Russian text: "Привет мир!"
    let text_russian = "Привет мир!".as_bytes();
    // English text: "Hello world!"
    let text_english = b"Hello world!";

    // Russian prefilter OLD: search for "П" (bytes [208, 159]) using memmem
    let prefilter_russian_old = Prefilter::LiteralWithOffset {
        needle: vec![208, 159],
        max_offset: 1,
    };

    // Russian prefilter NEW: search for "П" using TwoByteLiteral (memchr + check)
    let prefilter_russian = Prefilter::TwoByteLiteral {
        byte1: 208,
        byte2: 159,
        max_offset: 1,
    };

    // English prefilter options
    let prefilter_english_single = Prefilter::SingleByte {
        byte: b'H',
        max_offset: 1,
    };

    let prefilter_english_multi = Prefilter::MultiBytes {
        bytes: vec![b'H', b'h', b'e', b'E'],
        max_offset: 1,
    };

    let prefilter_english_two = Prefilter::TwoBytes {
        byte1: b'H',
        byte2: b'e',
        max_offset: 1,
    };

    // Warmup
    for _ in 0..1000 {
        let _ = prefilter_russian.find_candidates(text_russian).next();
        let _ = prefilter_english_single.find_candidates(text_english).next();
    }

    // Profile Russian OLD (LiteralWithOffset/memmem)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = prefilter_russian_old.find_candidates(text_russian).next();
    }
    let russian_old_time = elapsed_ns(start, iterations);

    // Profile Russian NEW (TwoByteLiteral/memchr)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = prefilter_russian.find_candidates(text_russian).next();
    }
    let russian_time = elapsed_ns(start, iterations);

    // Profile English SingleByte
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = prefilter_english_single.find_candidates(text_english).next();
    }
    let english_single_time = elapsed_ns(start, iterations);

    // Profile English MultiBytes
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = prefilter_english_multi.find_candidates(text_english).next();
    }
    let english_multi_time = elapsed_ns(start, iterations);

    // Profile English TwoBytes (memchr2)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = prefilter_english_two.find_candidates(text_english).next();
    }
    let english_two_time = elapsed_ns(start, iterations);

    println!("=== Prefilter Performance (short text) ===");
    println!("Russian OLD (memmem):      {russian_old_time:.1} ns");
    println!("Russian NEW (TwoByteLit):  {russian_time:.1} ns");
    println!("Russian speedup:           {:.1}x", russian_old_time / russian_time);
    println!("English SingleByte:        {english_single_time:.1} ns");
    println!("English MultiBytes:        {english_multi_time:.1} ns");
    println!("English TwoBytes:          {english_two_time:.1} ns");
}

fn profile_long_text() {
    println!("\n=== Prefilter Performance (longer text) ===");
    let long_russian = "Это тестовая строка на русском языке. "
        .repeat(10)
        .into_bytes();
    let long_english = b"This is a test string in English language. ".repeat(10);

    // Russian: search for rare char at end
    let long_russian_text = [long_russian.as_slice(), "Привет!".as_bytes()].concat();
    // English: search for char at end
    let long_english_text = [long_english.as_slice(), b"Hello!"].concat();

    let prefilter_russian = Prefilter::TwoByteLiteral {
        byte1: 208,
        byte2: 159,
        max_offset: 1,
    };

    let prefilter_english_single = Prefilter::SingleByte {
        byte: b'H',
        max_offset: 1,
    };

    let long_iterations: u32 = 100_000;

    let start = Instant::now();
    for _ in 0..long_iterations {
        let _ = prefilter_russian.find_candidates(&long_russian_text).next();
    }
    let russian_long = elapsed_ns(start, long_iterations);

    let start = Instant::now();
    for _ in 0..long_iterations {
        let _ = prefilter_english_single
            .find_candidates(&long_english_text)
            .next();
    }
    let english_long = elapsed_ns(start, long_iterations);

    println!(
        "Russian LiteralWithOffset ({}b): {:.1} ns",
        long_russian_text.len(),
        russian_long
    );
    println!(
        "English SingleByte ({}b):        {:.1} ns",
        long_english_text.len(),
        english_long
    );
}

fn main() {
    let iterations = 1_000_000;

    profile_short_text(iterations);
    profile_long_text();
}
