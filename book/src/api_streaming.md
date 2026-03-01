# Streaming API

Process large data incrementally without loading into memory.

## Creating a Stream

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();
    println!("Stream created");
}
```

## Feeding Data

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();

    // Feed data in chunks
    for m in stream.feed(b"some hay and niddle here") {
        println!("Found '{}' at {}", m.as_str(), m.start());
    }
}
```

## Position Tracking

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();

    stream.feed(b"hello");
    assert_eq!(stream.position(), 5);

    stream.feed(b" world");
    assert_eq!(stream.position(), 11);
}
```

## Cross-Boundary Matches

Matches can span chunk boundaries:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();

    // First chunk - no match yet
    let _ = stream.feed(b"xxx ");

    // Second chunk - match spanning boundary
    let matches: Vec<_> = stream.feed(b"hello").collect();
    println!("Matches: {}", matches.len());
}
```

## Resetting

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();

    stream.feed(b"hello world");
    stream.reset();

    assert_eq!(stream.position(), 0);
}
```

## Finish

Check for remaining matches at end of stream:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let mut stream = re.stream();
    stream.feed(b"test hel");

    // Check remaining buffer
    let final_match = stream.finish();
}
```

## Reader Integration

Process files or other readers:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    use std::io::BufReader;
    use std::fs::File;

    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let file = File::open("Cargo.toml").unwrap();

    for m in re.stream().search_reader(BufReader::new(file)) {
        println!("Match at byte {}", m.start());
    }
}
```

## Byte-Level API

Search byte slices directly:

```rust
fn main() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    if let Some(m) = re.find_bytes(b"hello world") {
        println!("Found at {}-{}", m.start(), m.end());
    }

    // Iterate over bytes
    let matches: Vec<_> = re.find_iter_bytes(b"hello hullo").collect();
    println!("Matches: {}", matches.len());
}
```
