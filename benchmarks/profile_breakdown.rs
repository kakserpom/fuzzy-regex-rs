//! Profile breakdown of string operations for Russian vs English

use std::time::Instant;

/// Calculate elapsed time in nanoseconds per iteration
fn elapsed_ns(start: Instant, iterations: u32) -> f64 {
    start.elapsed().as_secs_f64() * 1_000_000_000.0 / f64::from(iterations)
}

fn profile_from_utf8(iterations: u32) {
    let russian_bytes: &[u8] = "Привет".as_bytes();
    let english_bytes: &[u8] = b"Hello";

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = std::str::from_utf8(russian_bytes);
    }
    let russian_from_utf8 = elapsed_ns(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = std::str::from_utf8(english_bytes);
    }
    let english_from_utf8 = elapsed_ns(start, iterations);

    println!("str::from_utf8:");
    println!("  Russian 12 bytes: {russian_from_utf8:.1} ns");
    println!("  English 5 bytes: {english_from_utf8:.1} ns");
    println!("  Ratio: {:.2}x\n", russian_from_utf8 / english_from_utf8);
}

fn profile_chars_count(iterations: u32) {
    let russian_str = "Привет";
    let english_str = "Hello";

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = russian_str.chars().count();
    }
    let russian_char_count = elapsed_ns(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = english_str.chars().count();
    }
    let english_char_count = elapsed_ns(start, iterations);

    println!("chars().count():");
    println!("  Russian 6 chars: {russian_char_count:.1} ns");
    println!("  English 5 chars: {english_char_count:.1} ns");
    println!("  Ratio: {:.2}x\n", russian_char_count / english_char_count);
}

fn profile_len(iterations: u32) {
    let english_str = "Hello";

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = english_str.len();
    }
    let english_len = elapsed_ns(start, iterations);

    println!("len() for ASCII:");
    println!("  English: {english_len:.1} ns\n");
}

fn profile_is_ascii(iterations: u32) {
    let russian_str = "Привет";
    let english_str = "Hello";

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = russian_str.is_ascii();
    }
    let russian_is_ascii = elapsed_ns(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = english_str.is_ascii();
    }
    let english_is_ascii = elapsed_ns(start, iterations);

    println!("is_ascii():");
    println!("  Russian: {russian_is_ascii:.1} ns");
    println!("  English: {english_is_ascii:.1} ns");
    println!("  Ratio: {:.2}x\n", russian_is_ascii / english_is_ascii);
}

fn profile_char_iteration(iterations: u32) {
    let russian_str = "Привет";
    let english_str = "Hello";

    let start = Instant::now();
    for _ in 0..iterations {
        for c in russian_str.chars() {
            std::hint::black_box(c);
        }
    }
    let russian_chars = elapsed_ns(start, iterations);

    let start = Instant::now();
    for _ in 0..iterations {
        for c in english_str.chars() {
            std::hint::black_box(c);
        }
    }
    let english_chars = elapsed_ns(start, iterations);

    println!("char iteration:");
    println!("  Russian 6 chars: {russian_chars:.1} ns");
    println!("  English 5 chars: {english_chars:.1} ns");
    println!("  Ratio: {:.2}x", russian_chars / english_chars);
}

fn main() {
    println!("=== Edit Breakdown Profiling ===\n");

    let iterations = 1_000_000;

    profile_from_utf8(iterations);
    profile_chars_count(iterations);
    profile_len(iterations);
    profile_is_ascii(iterations);
    profile_char_iteration(iterations);
}
