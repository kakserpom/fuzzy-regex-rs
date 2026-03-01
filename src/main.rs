//! Demo for fuzzy-regex crate.

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: fuzzy_regex::MiMalloc = fuzzy_regex::MiMalloc;

use fuzzy_regex::{FuzzyRegex, FuzzyRegexBuilder};

fn main() {
    println!("=== Fuzzy Regex Demo ===\n");

    // Simple exact matching
    println!("1. Simple exact matching:");
    let re = FuzzyRegex::new(r"hello").unwrap();
    println!("   Pattern: 'hello'");
    println!("   'hello world' matches: {}", re.is_match("hello world"));
    println!("   'say hello' matches: {}", re.is_match("say hello"));
    println!();

    // Fuzzy matching with edits
    println!("2. Fuzzy matching (hello~2 allows 2 edits):");
    let re = FuzzyRegexBuilder::new(r"hello~2")
        .similarity(0.6)
        .build()
        .unwrap();
    println!("   Pattern: 'hello~2' with threshold 0.6");
    println!("   'hello' matches: {}", re.is_match("hello"));
    println!("   'helo' matches: {}", re.is_match("helo"));
    println!("   'helllo' matches: {}", re.is_match("helllo"));
    println!();

    // Character classes and quantifiers
    println!("3. Character classes and quantifiers:");
    let re = FuzzyRegex::new(r"[a-z]+\d+").unwrap();
    println!("   Pattern: '[a-z]+\\d+'");
    println!("   'abc123' matches: {}", re.is_match("abc123"));
    println!("   'test42' matches: {}", re.is_match("test42"));
    println!("   '123abc' matches: {}", re.is_match("123abc"));
    println!();

    // Capture groups
    println!("4. Capture groups:");
    let re = FuzzyRegex::new(r"(\w+)@(\w+)\.(\w+)").unwrap();
    println!("   Pattern: '(\\w+)@(\\w+)\\.(\\w+)'");
    if let Some(caps) = re.captures("user@example.com") {
        println!("   Input: 'user@example.com'");
        println!("   Full match: '{}'", caps.get(0).unwrap().as_str());
        println!("   Group 1: '{}'", caps.get(1).unwrap().as_str());
        println!("   Group 2: '{}'", caps.get(2).unwrap().as_str());
        println!("   Group 3: '{}'", caps.get(3).unwrap().as_str());
    }
    println!();

    // Named groups
    println!("5. Named capture groups:");
    let re = FuzzyRegex::new(r"(?<name>\w+): (?<value>\d+)").unwrap();
    println!("   Pattern: '(?<name>\\w+): (?<value>\\d+)'");
    if let Some(caps) = re.captures("count: 42") {
        println!("   Input: 'count: 42'");
        println!("   name: '{}'", caps.name("name").unwrap().as_str());
        println!("   value: '{}'", caps.name("value").unwrap().as_str());
    }
    println!();

    // Alternation
    println!("6. Alternation:");
    let re = FuzzyRegex::new(r"cat|dog|bird").unwrap();
    println!("   Pattern: 'cat|dog|bird'");
    println!("   'I have a cat' matches: {}", re.is_match("I have a cat"));
    println!("   'I have a dog' matches: {}", re.is_match("I have a dog"));
    println!(
        "   'I have a fish' matches: {}",
        re.is_match("I have a fish")
    );
    println!();

    // Replace
    println!("7. Replace:");
    let re = FuzzyRegex::new(r"world").unwrap();
    let result = re.replace("hello world", "Rust");
    println!("   Pattern: 'world'");
    println!("   Input: 'hello world'");
    println!("   Result: '{result}'");
    println!();

    // Replace all
    println!("8. Replace all:");
    let re = FuzzyRegex::new(r"\d+").unwrap();
    let result = re.replace_all("a1b2c3", "X");
    println!("   Pattern: '\\d+'");
    println!("   Input: 'a1b2c3'");
    println!("   Result: '{result}'");
    println!();

    // Split
    println!("9. Split:");
    let re = FuzzyRegex::new(r"[,;]+").unwrap();
    let parts: Vec<_> = re.split("a,b;c,,d").collect();
    println!("   Pattern: '[,;]+'");
    println!("   Input: 'a,b;c,,d'");
    println!("   Parts: {parts:?}");
    println!();

    // Find all matches
    println!("10. Find all matches:");
    let re = FuzzyRegex::new(r"\w+").unwrap();
    let matches: Vec<_> = re
        .find_iter("hello world 123")
        .map(|m| m.as_str())
        .collect();
    println!("   Pattern: '\\w+'");
    println!("   Input: 'hello world 123'");
    println!("   Matches: {matches:?}");

    println!("\n=== Demo Complete ===");
}
