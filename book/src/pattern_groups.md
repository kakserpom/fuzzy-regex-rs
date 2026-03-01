# Groups and Alternation

Group patterns and match alternatives.

## Capture Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"(?<user>\w+)@(?<domain>\w+\.\w+)").unwrap();
    let caps = re.captures("john@example.com").unwrap();

    assert_eq!(caps.name("user").unwrap().as_str(), "john");
    assert_eq!(caps.name("domain").unwrap().as_str(), "example.com");
}
```

## Numbered Groups

```rust
fn main() {
    let re = FuzzyRegex::new(r"(\w+)@(\w+)").unwrap();
    let caps = re.captures("test@example").unwrap();

    assert_eq!(caps.get(1).unwrap().as_str(), "test");
    assert_eq!(caps.get(2).unwrap().as_str(), "example");
}
```

## Non-Capturing Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // (?:...) doesn't create a capture group
    let re = FuzzyRegex::new("(?:http|https)://").unwrap();
    println!("{}", re.is_match("http://example.com"));
}
```

## Alternation

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match either option
    let re = FuzzyRegex::new("(foo|bar)").unwrap();

    assert!(re.is_match("foo"));
    assert!(re.is_match("bar"));
    assert!(!re.is_match("baz"));
}
```

## Nested Groups

```rust
fn main() {
    let re = FuzzyRegex::new(r"((ab)(cd))").unwrap();
    let caps = re.captures("abcd").unwrap();

    assert_eq!(caps.get(0).unwrap().as_str(), "abcd"); // Full match
    assert_eq!(caps.get(1).unwrap().as_str(), "abcd"); // Outer group
    assert_eq!(caps.get(2).unwrap().as_str(), "ab");   // First inner
    assert_eq!(caps.get(3).unwrap().as_str(), "cd");   // Second inner
}
```

## Fuzzy Groups

Apply fuzziness to groups:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("((?:hello){e<=1})").unwrap();
    println!("{}", re.is_match("helo"));
}
```
