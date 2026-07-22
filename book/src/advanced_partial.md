# Partial Matching

Match incomplete or streaming input.

## Enabling Partial Matching

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(true)
        .build()
        .unwrap();
    
    println!("Created");
}
```

## How It Works

When enabled, matches that reach the end of the input are marked as "partial":

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(true)
        .build()
        .unwrap();

    // Match at end of text - partial
    let m1 = re.find("hello").unwrap();
    assert!(m1.partial());

    // Match not at end - not partial ("hello" is followed by more text)
    let m2 = re.find("say hello world").unwrap();
    assert!(!m2.partial());

    // Fuzzy match reaching end - also partial
    let m3 = re.find("hallo").unwrap();
    assert!(m3.partial());
}
```

## Use Cases

### Streaming Input

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:command){e<=1}")
        .partial(true)
        .build()
        .unwrap();

    let mut stream = re.stream();

    // Feed incomplete data; matches are reported by their byte span as more
    // data arrives across chunks.
    for m in stream.feed(b"cmd") {
        println!("Match at {}-{}", m.start(), m.end());
    }
}
```

### Progressive Matching

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(true)
        .build()
        .unwrap();

    // Check if more input needed
    let m = re.find("incomplete");
    if m.map(|m| m.partial()).unwrap_or(false) {
        // Get more input
    }
}
```

## Without Partial

By default (`partial(false)`), partial matches behave the same as full matches:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(false) // default
        .build()
        .unwrap();

    let m = re.find("hello").unwrap();
    assert!(!m.partial()); // Always false
}
```
