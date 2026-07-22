# Anchors and Boundaries

Match positions in the string.

## String Anchors

| Syntax | Description |
|--------|-------------|
| `^` | Start of string |
| `$` | End of string |

```rust
fn main() {
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
}
```

> **Performance:** A fuzzy pattern ending in `$` (single-line mode) is
> searched only in a small window near the end of the input rather than
> scanned position by position, so its cost stays flat as the text grows.
> See [Performance Tips](./perf_tips.md#5-end-anchor-fuzzy-patterns).

## Word Boundaries

| Syntax | Description |
|--------|-------------|
| `\b` | Word boundary |
| `\B` | Non-word boundary |

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match whole word "cat"
    let re1 = FuzzyRegex::new(r"\bcat\b").unwrap();
    assert!(re1.is_match("cat"));
    assert!(re1.is_match("the cat sat"));
    assert!(!re1.is_match("category"));

    // Match "cat" only when NOT at a word boundary (i.e. inside a word)
    let re2 = FuzzyRegex::new(r"\Bcat\B").unwrap();
    assert!(re2.is_match("concatenate")); // "cat" is mid-word
    assert!(!re2.is_match("cat"));        // standalone word - boundaries on both sides
    assert!(!re2.is_match("category"));   // "cat" is at the start (a boundary)
}
```

## Multi-line Mode

With `(?m)`, `^` and `$` match line boundaries:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?m)^hello$").unwrap();
    let text = "hello\nhello\nhello";
    let matches: Vec<_> = re.find_iter(text).collect();
    assert_eq!(matches.len(), 3);
}
```
