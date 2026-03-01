# Atomic Groups and Possessive Quantifiers

Advanced features for performance and pattern control.

## Atomic Groups `(?>...)`

Atomic groups prevent backtracking once matched:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Without atomic group: can match "abc" then backtrack
    let re1 = FuzzyRegex::new("(?:ab|abc)").unwrap();

    // Atomic: once "abc" matches, no backtracking
    let re2 = FuzzyRegex::new("(?>ab|abc)").unwrap();
    
    println!("re1: {}", re1.is_match("abc"));
    println!("re2: {}", re2.is_match("abc"));
}
```

## Possessive Quantifiers

Like atomic groups for quantifiers:

| Syntax | Description             |
|--------|-------------------------|
| `*+`   | Possessive zero-or-more |
| `++`   | Possessive one-or-more  |
| `?+`   | Possessive zero-or-one  |

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Normal: backtracks to find match
    let re1 = FuzzyRegex::new("a*b").unwrap();

    // Possessive: doesn't backtrack
    let re2 = FuzzyRegex::new("a*+b").unwrap();
    
    println!("re1: {}", re1.is_match("ab"));
    println!("re2: {}", re2.is_match("ab"));
}
```

## When to Use

Atomic groups and possessive quantifiers are useful when:

1. **Performance**: Prevent exponential backtracking
2. **Determinism**: Control match behavior explicitly
3. **Greedy matching**: When you want maximum match without backtracking

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Pattern that could cause backtracking issues:
    // Without possessive: "aaaaaaaa" + "a*" + "b" can be slow
    let re = FuzzyRegex::new("(?:a++)b").unwrap();
    println!("{}", re.is_match("ab"));
}
```

## Match Reset `\K`

Reset the start of the match:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // \K resets match start - keeps "prefix" out of match
    let re = FuzzyRegex::new(r"prefix\Kworld").unwrap();
    let m = re.find("prefixworld").unwrap();
    assert_eq!(m.as_str(), "world");
    assert_eq!(m.start(), 6); // Starts at "world", not "prefix"
}
```
