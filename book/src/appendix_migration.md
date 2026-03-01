# Migration Guide

Migrating from other libraries to fuzzy-regex.

## From regex (standard)

fuzzy-regex extends the standard `regex` crate with fuzzy matching:

```rust
// Standard regex
let re = regex::new("hello").unwrap();

// fuzzy-regex
let re = fuzzy_regex::FuzzyRegex::new("hello").unwrap();

// Add fuzziness
let re = fuzzy_regex::FuzzyRegex::new("(?:hello){e<=1}").unwrap();
```

## From fuzzy-aho-corasick

See [Compatibility Layer](../advanced_compat.md) for a drop-in replacement.

## From mrab-regex

The fuzzy syntax is compatible:

```rust
// mrab-regex
let re = regex::new(r"(?i)(?:hello){e<=1}");

// fuzzy-regex (same syntax)
let re = FuzzyRegex::new(r"(?i)(?:hello){e<=1}").unwrap();
```
