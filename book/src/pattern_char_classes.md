# Character Classes

Define sets of characters to match.

## Basic Classes

```rust
use fuzzy_regex::FuzzyRegex;

// Match any vowel
let re1 = FuzzyRegex::new("[aeiou]").unwrap();

// Match any consonant
let re2 = FuzzyRegex::new("[^aeiou]").unwrap();

// Match any lowercase letter
let re3 = FuzzyRegex::new("[a-z]").unwrap();

// Match any alphanumeric
let re4 = FuzzyRegex::new("[a-zA-Z0-9]").unwrap();
```

## Predefined Classes

By default, `\w`, `\d`, `\s` match ASCII characters only:

| Escape | ASCII Only | Unicode (with `(?u)` |
|--------|-----------|---------------------|
| `\d` | `[0-9]` | Unicode digits |
| `\D` | `[^0-9]` | Non-digit |
| `\w` | `[a-zA-Z0-9_]` | Unicode word chars |
| `\W` | `[^a-zA-Z0-9_]` | Non-word |
| `\s` | `[ \t\n\r\f]` | Unicode whitespace |
| `\S` | `[^ \t\n\r\f]` | Non-whitespace |

## Unicode Mode

Enable Unicode character classes with `(?u)` flag:

```rust
use fuzzy_regex::FuzzyRegexBuilder;

// Via inline flag
let re1 = FuzzyRegex::new("(?u)\\w+").unwrap();
assert!(re1.is_match("привет")); // Cyrillic matched

// Via builder
let re2 = FuzzyRegexBuilder::new("\\w+")
    .unicode(true)
    .build()
    .unwrap();
```

## Unicode Property Escapes

```rust
use fuzzy_regex::FuzzyRegex;

// Unicode letters
let re1 = FuzzyRegex::new(r"\p{L}").unwrap();

// Unicode digits
let re2 = FuzzyRegex::new(r"\p{N}").unwrap();

// Emoji
let re3 = FuzzyRegex::new(r"\p{Emoji}").unwrap();
```

## Nested Classes

```rust
// POSIX-like classes (inside character class)
let re = FuzzyRegex::new("[[:alpha:]]").unwrap();
```
