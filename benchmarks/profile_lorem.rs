use fuzzy_regex::FuzzyRegex;

fn main() {
    // Simple test case
    let text = "Lorem";
    let re = FuzzyRegex::new("(?:Lorem){e<=2}").unwrap();

    // Time it
    let iters = 100000;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        let _ = re.find(text);
    }
    let elapsed = start.elapsed();
    println!("Text: '{}' ({} bytes)", text, text.len());
    println!("Total: {:?} for {} iters", elapsed, iters);
    println!(
        "Per iter: {:.0} ns",
        elapsed.as_nanos() as f64 / iters as f64
    );

    // Now test with longer text where match is at position 0
    let text2 = "Lorem ipsum dolor sit amet";
    let start = std::time::Instant::now();
    for _ in 0..iters {
        let _ = re.find(text2);
    }
    let elapsed = start.elapsed();
    println!("\nText2: '{}' ({} bytes)", text2, text2.len());
    println!(
        "Per iter: {:.0} ns",
        elapsed.as_nanos() as f64 / iters as f64
    );

    // Test with text where match is NOT at position 0
    let text3 = "xxx Lorem";
    let start = std::time::Instant::now();
    for _ in 0..iters {
        let _ = re.find(text3);
    }
    let elapsed = start.elapsed();
    println!("\nText3: '{}' ({} bytes)", text3, text3.len());
    println!(
        "Per iter: {:.0} ns",
        elapsed.as_nanos() as f64 / iters as f64
    );
}
