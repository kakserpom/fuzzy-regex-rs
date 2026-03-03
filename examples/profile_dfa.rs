use fuzzy_regex::FuzzyRegex;

fn main() {
    let patterns = [r"\bquick\b", r"quick", r"\d+", r"quick.*fox"];

    for pattern in patterns {
        let re = FuzzyRegex::new(pattern).unwrap();

        // Check if DFA is available
        // We can't directly access internal state, but we can check by performance
        let text = "The quick brown fox jumps over the lazy dog.";

        if let Some(m) = re.find(text) {
            println!(
                "'{}' matches '{}' at {}-{}",
                pattern,
                m.as_str(),
                m.start(),
                m.end()
            );
        } else {
            println!("'{}' no match", pattern);
        }
    }
}
