# FuzzyRegex

The main compiled regex type.

## Construction

```rust
use fuzzy_regex::FuzzyRegex;

// Simple construction
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

// With error handling
let re = match FuzzyRegex::new(pattern) {
    Ok(r) => r,
    Err(e) => panic!("Invalid pattern: {}", e),
};
```

## Search Methods

### `is_match`

```rust
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

re.is_match("hello"); // true
re.is_match("hallo"); // true
re.is_match("world"); // false
```

### `find`

```rust
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

if let Some(m) = re.find("say hello world") {
    println!("Found '{}' at {}-{}", m.as_str(), m.start(), m.end());
    println!("Similarity: {:.2}", m.similarity());
}
```

### `find_iter`

```rust
let re = FuzzyRegex::new(r"(?:\w+){e<=1}").unwrap();

for m in re.find_iter("test tset tsat") {
    println!("Match: '{}'", m.as_str());
}
```

### `find_rev`

Find rightmost match:

```rust
let re = FuzzyRegex::new("(?:foo)").unwrap();
let m = re.find_rev("foo foo foo").unwrap();
assert_eq!(m.start(), 8);
```

## Replace Methods

```rust
let re = FuzzyRegex::new("world").unwrap();

// Replace first match
let result = re.replace("hello world", "rust");
assert_eq!(result, "hello rust");

// Replace all matches
let result = re.replace_all("hello world world", "rust");
assert_eq!(result, "hello rust rust");
```

## Other Methods

```rust
let re = FuzzyRegex::new(r"(?<word>\w+)").unwrap();

// Capture groups
let caps = re.captures("hello").unwrap();
let word = caps.name("word").unwrap();

// Split
let parts: Vec<_> = re.split("a1b2c").collect();

// Check pattern info
println!("Capture count: {}", re.capture_count());
```
