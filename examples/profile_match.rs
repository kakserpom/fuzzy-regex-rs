use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let text = "Lorem ipsum dolor sit amet";
    let re = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();

    // Single match timing
    let start = Instant::now();
    let m = re.find(text);
    let elapsed = start.elapsed();

    println!("Text: '{}' ({} bytes)", text, text.len());
    println!("Pattern: Lorem with e<=2");
    if let Some(m) = m {
        println!("Found: '{}' at {}-{}", m.as_str(), m.start(), m.end());
    }
    println!("Time: {:.2} us", elapsed.as_nanos() as f64 / 1000.0);

    // Check what's happening internally by testing without fuzzy
    let re_exact = FuzzyRegex::new("Lorem").unwrap();
    let start2 = Instant::now();
    let _ = re_exact.find(text);
    let elapsed2 = start2.elapsed();
    println!(
        "\nExact 'Lorem': {:.2} us",
        elapsed2.as_nanos() as f64 / 1000.0
    );
}
