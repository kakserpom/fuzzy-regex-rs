# Lookahead and Lookbehind

Assert what comes before or after the match without including it.

## Lookahead

### Positive Lookahead `(?=...)`

Match only if followed by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "hello" only if followed by " world"
    let re = FuzzyRegex::new("hello(?= world)").unwrap();

    assert!(re.is_match("hello world"));  // Matches "hello"
    assert!(!re.is_match("hello there")); // No match
}
```

### Negative Lookahead `(?!...)`

Match only if NOT followed by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "hello" only if NOT followed by " world"
    let re = FuzzyRegex::new("hello(?! world)").unwrap();

    assert!(re.is_match("hello there")); // Matches "hello"
    assert!(!re.is_match("hello world")); // No match
}
```

## Lookbehind

### Positive Lookbehind `(?<=...)`

Match only if preceded by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "world" only if preceded by "hello "
    let re = FuzzyRegex::new("(?<=hello )world").unwrap();

    assert!(re.is_match("hello world"));
    assert!(!re.is_match("bye world"));

    // Get match position
    let m = re.find("say hello world here").unwrap();
    assert_eq!(m.start(), 9); // After "hello "
}
```

### Negative Lookbehind `(?<!...)`

Match only if NOT preceded by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "world" only if NOT preceded by "hello "
    let re = FuzzyRegex::new("(?<!hello )world").unwrap();

    assert!(re.is_match("bye world"));
    assert!(!re.is_match("hello world"));
}
```

## Fuzzy Lookbehind

Lookbehind can include fuzzy matching:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "world" preceded by "hello" with up to 1 error
    let re = FuzzyRegex::new("(?<=(?:hello){e<=1})world").unwrap();

    assert!(re.is_match("hello world")); // Exact
    assert!(re.is_match("hallo world")); // 1 substitution
    assert!(!re.is_match("goodbye world")); // Too different
}
```

## Variable-Length Lookbehind

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Variable-length lookbehind (alternation)
    let re = FuzzyRegex::new("(?<=(?:hello|hi) )world").unwrap();
    println!("{}", re.is_match("hi world"));
}
```
