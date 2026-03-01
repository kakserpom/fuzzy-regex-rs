use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    println!("=== Profile Lookbehind ===\n");

    // Test with medium size text
    let text = format!("{}hello world", "x".repeat(500));
    println!("Text size: {} bytes\n", text.len());

    // Profile compilation
    let start = Instant::now();
    let regex = FuzzyRegex::new(r"(?<=hello )world").unwrap();
    println!("Compilation: {:?}", start.elapsed());

    // Profile first match
    let start = Instant::now();
    let _ = regex.find(&text);
    println!("First find: {:?}", start.elapsed());

    // Profile subsequent matches
    let start = Instant::now();
    for _ in 0..10 {
        std::hint::black_box(regex.find(&text));
    }
    println!("10 more finds: {:?} ({:?} per find)", start.elapsed(), start.elapsed() / 10);

    // Now test mrab-regex comparison pattern (without lookbehind)
    println!("\n--- Without lookbehind ---");
    let regex = FuzzyRegex::new(r"hello world").unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        std::hint::black_box(regex.find(&text));
    }
    println!("100 finds: {:?} ({:?} per find)", start.elapsed(), start.elapsed() / 100);

    // Test just "world"
    println!("\n--- Just 'world' ---");
    let regex = FuzzyRegex::new(r"world").unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        std::hint::black_box(regex.find(&text));
    }
    println!("100 finds: {:?} ({:?} per find)", start.elapsed(), start.elapsed() / 100);

    // Test character-by-character lookbehind (no fuzzy literals)
    println!("\n--- Single char lookbehind ---");
    let regex = FuzzyRegex::new(r"(?<=x)hello").unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        std::hint::black_box(regex.find(&text));
    }
    println!("100 finds: {:?} ({:?} per find)", start.elapsed(), start.elapsed() / 100);
}
