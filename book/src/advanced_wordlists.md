# Word Lists

Define named lists of patterns for reuse.

## Basic Usage

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let mut re = FuzzyRegex::new(r"\L<keywords>").unwrap();

    // Set the word list
    re.set_word_list(&["hello", "world", "test"]).unwrap();

    assert!(re.is_match("hello"));
    assert!(re.is_match("world"));
    assert!(!re.is_match("foo"));
}
```

## With Fuzzy Matching

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let mut re = FuzzyRegex::new(r"\L<keywords>{e<=1}").unwrap();
    re.set_word_list(&["hello", "world"]).unwrap();

    assert!(re.is_match("hallo")); // "hello" with 1 error
    assert!(re.is_match("wrold")); // "world" with 1 error
}
```

## Multiple Word Lists

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"\L<names>|\L<places>").unwrap();
    // Note: Multiple lists require builder pattern
    println!("Created");
}
```

## Use Cases

- **Keyword matching**: Match against a list of keywords
- **Name matching**: Match against database of names
- **Dictionary lookup**: Match words from a dictionary
