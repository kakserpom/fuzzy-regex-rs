use fuzzy_regex::FuzzyRegex;

fn main() {
    let patterns = [r"\bquick\b", r"\bquick", r"quick\b"];

    for pattern in patterns {
        let re = FuzzyRegex::new(pattern).unwrap();
        let text = "The quick brown fox jumps.";

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

        // Also test find_iter
        let matches: Vec<_> = re.find_iter(text).collect();
        println!("  find_iter: {} matches", matches.len());
    }
}
