use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let text = "The quick brown fox jumps over the lazy dog.";
    let long_text = text.repeat(100);

    println!("fuzzy-regex benchmarks (us/iter)");
    println!("================================");

    // Short text
    let re = FuzzyRegex::new("(?:quikc){e<=1}").unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let _ = re.find(text);
    }
    println!(
        "short text, 1 edit:    {:>8.1}",
        start.elapsed().as_micros() as f64 / 1000.0
    );

    let re = FuzzyRegex::new("(?:qwick){e<=2}").unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let _ = re.find(text);
    }
    println!(
        "short text, 2 edits:   {:>8.1}",
        start.elapsed().as_micros() as f64 / 1000.0
    );

    // Long text
    let re = FuzzyRegex::new("(?:lorem){e<=2}").unwrap();
    let start = Instant::now();
    for _ in 0..10 {
        let _ = re.find(&long_text);
    }
    println!(
        "long text, 2 edits:    {:>8.1}",
        start.elapsed().as_micros() as f64 / 10.0
    );

    let re = FuzzyRegex::new("(?:xyzzy){e<=1}").unwrap();
    let start = Instant::now();
    for _ in 0..10 {
        let _ = re.find(&long_text);
    }
    println!(
        "long text, no match:   {:>8.1}",
        start.elapsed().as_micros() as f64 / 10.0
    );
}
