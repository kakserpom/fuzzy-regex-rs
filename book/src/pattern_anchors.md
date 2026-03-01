# Anchors and Boundaries

Match positions in the string.

## String Anchors

| Syntax | Description |
|--------|-------------|
| `^` | Start of string |
| `$` | End of string |

```rust
use fuzzy_regex::FuzzyRegex;

// Match at start
let re1 = FuzzyRegex::new("^hello").unwrap();
assert!(re1.is_match("hello world"));
assert!(!re1.is_match("say hello"));

// Match at end
let re2 = FuzzyRegex::new("hello$").unwrap();
assert!(re2.is_match("say hello"));
assert!(!re2.is_match("hello world"));

// Match entire string
let re3 = FuzzyRegex::new("^hello$").unwrap();
assert!(re3.is_match("hello"));
assert!(!re3.is_match("hello world"));
```

## Word Boundaries

| Syntax | Description |
|--------|-------------|
| `\b` | Word boundary |
| `\B` | Non-word boundary |

```rust
use fuzzy_regex::FuzzyRegex;

// Match whole word "cat"
let re1 = FuzzyRegex::new(r"\bcat\b").unwrap();
assert!(re1.is_match("cat"));
assert!(re1.is_match("the cat sat"));
assert!(!re1.is_match("category"));

// Match "cat" not at word boundary
let re2 = FuzzyRegex::new(r"\Bcat\B").unwrap();
assert!(re2.is_match("category"));
assert!(!re2.is_match("cat"));
```

## Multi-line Mode

With `(?m)`, `^` and `$` match line boundaries:

```rust
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new("(?m)^hello$").unwrap();
let text = "hello\nhello\nhello";
let matches: Vec<_> = re.find_iter(text).collect();
assert_eq!(matches.len(), 3);
```
