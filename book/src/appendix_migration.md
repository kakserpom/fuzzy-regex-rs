# Migration Guide

Migrating from other libraries to fuzzy-regex.

## From regex (standard)

fuzzy-regex extends the standard `regex` crate with fuzzy matching:

```rust
fn main() {
    // Standard regex (the `regex` crate):
    //   let re = regex::Regex::new("hello").unwrap();

    // fuzzy-regex
    let re = fuzzy_regex::FuzzyRegex::new("hello").unwrap();

    // Add fuzziness
    let re = fuzzy_regex::FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    let _ = re;
    println!("Created");
}
```

## From fuzzy-aho-corasick

See [Compatibility Layer](../advanced_compat.md) for a drop-in replacement.

## From mrab-regex

The fuzzy syntax is compatible:

```rust
fn main() {
    // mrab-regex (Python):
    //   re = regex.compile(r"(?i)(?:hello){e<=1}")

    // fuzzy-regex (same syntax)
    let re = fuzzy_regex::FuzzyRegex::new(r"(?i)(?:hello){e<=1}").unwrap();

    let _ = re;
    println!("Created");
}
```
