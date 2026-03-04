use memchr::memmem;
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
    println!("{name:45} {ns:>8.1} ns",);
}

fn main() {
    let text = "The quick brown fox jumps over the lazy dog.";
    let literal = b"quick";

    println!("=== memchr overhead ===\n");

    // Pure memchr
    bench("memchr::memchr", 1000000, || {
        let _ = memchr::memchr(literal[0], text.as_bytes());
    });

    bench("memmem::find (5 bytes)", 1000000, || {
        let _ = memmem::find(text.as_bytes(), literal);
    });

    // Full FuzzyRegex call
    let re = fuzzy_regex::FuzzyRegex::new("quick").unwrap();

    bench("fuzzy find (cached)", 1000000, || {
        let _ = re.find(text);
    });

    println!();

    // Regex for comparison
    let re_regex = regex::Regex::new("quick").unwrap();
    bench("regex find", 1000000, || {
        let _ = re_regex.find(text);
    });
}
