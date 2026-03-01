# Match Results

Methods available on match results.

## Basic Methods

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let m = re.find("say hello world").unwrap();

    // Get matched string
    println!("{}", m.as_str());           // &str: "hello"
    println!("{}", m.start());            // usize: 4 (byte position)
    println!("{}", m.end());              // usize: 9
    println!("{}", m.len());              // usize: 5
    println!("{}", m.is_empty());         // bool: false
}
```

## Similarity

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let m = re.find("say hello world").unwrap();

    // Similarity score 0.0 - 1.0
    println!("{}", m.similarity());       // f32: 1.0 = exact, lower = more errors
}
```

## Partial Match

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let m = re.find("say hello").unwrap();

    // Check if match reaches end of input (for partial matching)
    println!("{}", m.partial());         // bool: true if match at end
}
```

## Match as Different Types

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let m = re.find("hello world").unwrap();

    // Get raw bytes
    let bytes = m.as_bytes();

    // Get range
    let range = m.range(); // Range<usize>
    println!("Range: {:?}", range);
}
```

## Iterating Matches

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"\w+").unwrap();

    for m in re.find_iter("hello world") {
        println!("{}", m.as_str());
    }
}
```

## Match Iteration with Positions

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:foo){e<=1}").unwrap();

    for m in re.find_iter("foo fxo fxoo") {
        println!("'{}' at [{}, {})", m.as_str(), m.start(), m.end());
    }
}
```
