# Quantifiers

Control how many times to match a pattern.

## Basic Quantifiers

| Syntax | Description | Example |
|--------|-------------|---------|
| `*` | Zero or more | `ab*` matches "a", "ab", "abb" |
| `+` | One or more | `ab+` matches "ab", "abb" |
| `?` | Zero or one | `ab?` matches "a", "ab" |
| `{n}` | Exactly n | `a{3}` matches "aaa" |
| `{n,}` | At least n | `a{2,}` matches "aa", "aaa" |
| `{n,m}` | Between n and m | `a{2,4}` matches "aa", "aaa", "aaaa" |

## Lazy Quantifiers

Add `?` to make quantifiers lazy (match as few as possible):

```rust
use fuzzy_regex::FuzzyRegex;

// Greedy: matches as much as possible
let re1 = FuzzyRegex::new(r"<.+>").unwrap();

// Lazy: matches as little as possible  
let re2 = FuzzyRegex::new(r"<.+?>").unwrap();

let text = "<tag>more</tag>";
assert!(re1.find(text).unwrap().as_str() == "<tag>more</tag>");
assert!(re2.find(text).unwrap().as_str() == "<tag>");
```

## Possessive Quantifiers

Prevent backtracking (useful for performance):

```rust
use fuzzy_regex::FuzzyRegex;

// a*+ doesn't backtrack
let re = FuzzyRegex::new("a*+b").unwrap();
```

## Quantifiers with Fuzzy Matching

```rust
use fuzzy_regex::FuzzyRegex;

// Match "ab" with up to 1 error, repeated 1-3 times
let re = FuzzyRegex::new("(?:ab){e<=1}{1,3}").unwrap();
```
