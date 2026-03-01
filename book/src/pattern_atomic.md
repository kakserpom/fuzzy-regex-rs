# Atomic Groups and Possessive Quantifiers

Advanced features for performance and pattern control.

## Atomic Groups `(?>...)`

Atomic groups prevent backtracking once matched:

```rust
use fuzzy_regex::FuzzyRegex;

// Without atomic group: can match "abc" then backtrack
let re1 = FuzzyRegex::new("(?:ab|abc)").unwrap();

// Atomic: once "abc" matches, no backtracking
let re2 = FuzzyRegex::new("(?>ab|abc)").unwrap();
```

## Possessive Quantifiers

Like atomic groups for quantifiers:

| Syntax | Description |
|--------|-------------|
| `*+` | Possessive zero-or-more |
| `++` | Possessive one-or-more |
| `?+` | Possessive zero-or-one |

```rust
use fuzzy_regex::FuzzyRegex;

// Normal: backtracks to find match
let re1 = FuzzyRegex::new("a*b").unwrap();

// Possessive: doesn't backtrack
let re2 = FuzzyRegex::new("a*+b").unwrap();
```

## When to Use

Atomic groups and possessive quantifiers are useful when:

1. **Performance**: Prevent exponential backtracking
2. **Determinism**: Control match behavior explicitly
3. **Greedy matching**: When you want maximum match without backtracking

```rust
// Pattern that could cause backtracking issues:
// Without possessive: "aaaaaaaa" + "a*" + "b" can be slow
let re = FuzzyRegex::new("(?:a++)b").unwrap();
```

## Match Reset `\K`

Reset the start of the match:

```rust
use fuzzy_regex::FuzzyRegex;

// \K resets match start - keeps "prefix" out of match
let re = FuzzyRegex::new("prefix\\Kworld").unwrap();
let m = re.find("prefixworld").unwrap();
assert_eq!(m.as_str(), "world");
assert_eq!(m.start(), 6); // Starts at "world", not "prefix"
```
