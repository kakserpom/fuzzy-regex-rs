# Pattern Syntax

fuzzy-regex supports standard regex syntax plus fuzzy matching.

## Quick Reference

| Syntax | Description |
|--------|-------------|
| `a` | Literal character |
| `.` | Any character except newline |
| `\d` | Digit [0-9] |
| `\D` | Non-digit |
| `\w` | Word character [a-zA-Z0-9_] |
| `\W` | Non-word character |
| `\s` | Whitespace |
| `\S` | Non-whitespace |
| `[abc]` | Character class |
| `[^abc]` | Negated class |
| `[a-z]` | Range |
| `^` | Start of string |
| `$` | End of string |
| `\b` | Word boundary |
| `\B` | Non-word boundary |

## Escaping

Special characters that need escaping: `.^$*+?{}[]\\|()`

```rust
use fuzzy_regex::FuzzyRegex;

// Match literal dot
let re = FuzzyRegex::new(r"file\.txt").unwrap();

// Match literal backslash
let re2 = FuzzyRegex::new(r"path\\to\\file").unwrap();
```
