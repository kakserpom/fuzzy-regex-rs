use fuzzy_regex::FuzzyRegex;

fn main() {
    let patterns = [
        (r"\bquick\b", "word boundary"),
        ("quick", "exact"),
        ("(?:quikc){e<=1}", "fuzzy"),
    ];

    for (pattern, desc) in patterns {
        let re = FuzzyRegex::new(pattern).unwrap();

        // Check what paths are taken
        println!("Pattern: '{}' ({})", pattern, desc);
    }
}
