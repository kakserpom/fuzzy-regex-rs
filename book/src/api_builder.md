# FuzzyRegexBuilder

Builder for customizing regex construction.

## Basic Usage

```rust
use fuzzy_regex::FuzzyRegexBuilder;

let re = FuzzyRegexBuilder::new("(?:hello)")
    .build()
    .unwrap();
```

## Builder Options

### Similarity Threshold

```rust
let re = FuzzyRegexBuilder::new("(?:hello){e<=2}")
    .similarity(0.8)  // Minimum similarity 0.0-1.0
    .build()
    .unwrap();
```

### Case Insensitivity

```rust
let re = FuzzyRegexBuilder::new("(?:hello)")
    .case_insensitive(true)
    .build()
    .unwrap();

assert!(re.is_match("HELLO"));
assert!(re.is_match("Hello"));
```

### Multi-line Mode

```rust
let re = FuzzyRegexBuilder::new("^hello$")
    .multi_line(true)  // ^ and $ match line boundaries
    .build()
    .unwrap();
```

### Dot-all Mode

```rust
let re = FuzzyRegexBuilder::new("a.b")
    .dot_all(true)  // . matches newlines
    .build()
    .unwrap();
```

### Partial Matching

```rust
let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
    .partial(true)  // Matches at end of text are partial
    .build()
    .unwrap();

let m = re.find("hello").unwrap();
assert!(m.partial()); // Match reaches end of text
```

### Timeout

```rust
use std::time::Duration;

let re = FuzzyRegexBuilder::new("(?:hello){e<=5}")
    .timeout(Duration::from_millis(100))
    .build()
    .unwrap();
```

## Chaining Options

```rust
let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
    .case_insensitive(true)
    .similarity(0.7)
    .partial(true)
    .multi_line(true)
    .build()
    .unwrap();
```
