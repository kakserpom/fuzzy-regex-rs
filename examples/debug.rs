use fuzzy_regex::FuzzyRegex;

fn main() {
    println!("=== Test unlimited errors {{e}} ===\n");

    // Test if {e} parses
    match FuzzyRegex::new("(?:foobar){e}") {
        Ok(re) => {
            println!("Parsed successfully!");
            println!("Testing 'foobar': {}", re.is_match("foobar"));
            println!("Testing 'xyzabc': {}", re.is_match("xyzabc"));
            println!("Testing 'anything': {}", re.is_match("anything"));
        }
        Err(e) => println!("Parse error: {:?}", e),
    }

    println!("\n=== Test {{e}} with per-type limits ===\n");
    match FuzzyRegex::new("(?:fuu){i<=3,d<=3,e}") {
        Ok(re) => {
            println!("Parsed successfully!");
            println!("Testing 'fuu': {}", re.is_match("fuu"));
            println!("Testing 'foo': {}", re.is_match("foo"));
        }
        Err(e) => println!("Parse error: {:?}", e),
    }
}
