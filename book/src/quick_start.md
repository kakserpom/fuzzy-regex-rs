# Quick Start

## Basic Fuzzy Matching

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Allow up to 2 edits
    let re = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

    assert!(re.is_match("hello"));   // Exact match
    assert!(re.is_match("helo"));    // 1 deletion
    assert!(re.is_match("helllo"));  // 1 insertion
    assert!(re.is_match("hallo"));   // 1 substitution
    assert!(re.is_match("hlelo"));   // 1 transposition
}
```

## Using the Builder

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:teh){e<=1}")
        .similarity(0.7)         // Minimum similarity score
        .case_insensitive(true)  // Case-insensitive matching
        .build()
        .unwrap();

    let m = re.find("I saw teh cat").unwrap();
    assert_eq!(m.as_str(), "teh");
    assert!(m.similarity() >= 0.7);
}
```

## Finding Matches

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"(?:test){e<=1}").unwrap();

    // Find the first fuzzy match ("tost" = "test" with 1 substitution)
    let m = re.find("run the tost suite").unwrap();
    assert_eq!(m.as_str(), "tost");

    // Find all fuzzy matches (each within 1 edit of "test")
    let matches: Vec<_> = re.find_iter("test tost tesr").collect();
    assert_eq!(matches.len(), 3);
}
```

## Streaming Large Data

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();

    // Process in chunks. A streaming match reports its byte span.
    for m in stream.feed(b"some hay and niddle here") {
        println!("Found match at {}-{}", m.start(), m.end());
    }

    // Check position
    assert_eq!(stream.position(), 24);
}
```
