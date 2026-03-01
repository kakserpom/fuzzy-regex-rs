# Quick Start

## Basic Fuzzy Matching

```rust
use fuzzy_regex::FuzzyRegex;

// Allow up to 2 edits
let re = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

assert!(re.is_match("hello"));   // Exact match
assert!(re.is_match("helo"));    // 1 deletion
assert!(re.is_match("helllo"));  // 1 insertion
assert!(re.is_match("hallo"));   // 1 substitution
assert!(re.is_match("hlelo"));   // 1 transposition
```

## Using the Builder

```rust
use fuzzy_regex::FuzzyRegexBuilder;

let re = FuzzyRegexBuilder::new("(?:teh){e<=1}")
    .similarity(0.7)         // Minimum similarity score
    .case_insensitive(true)  // Case-insensitive matching
    .build()
    .unwrap();

let m = re.find("I saw teh cat").unwrap();
assert_eq!(m.as_str(), "teh");
assert!(m.similarity() >= 0.7);
```

## Finding Matches

```rust
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new(r"(?:\w+){e<=1}").unwrap();

// Find first match
let m = re.find("This is a tset").unwrap();
assert_eq!(m.as_str(), "tset");

// Find all matches
let matches: Vec<_> = re.find_iter("test tset tsat").collect();
assert_eq!(matches.len(), 3);
```

## Streaming Large Data

```rust
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
let mut stream = re.stream();

// Process in chunks
for m in stream.feed(b"some hay and niddle here") {
    println!("Found: {} at {}", m.as_str(), m.start());
}

// Check position
assert_eq!(stream.position(), 24);
```
