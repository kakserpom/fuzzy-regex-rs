# Lookahead and Lookbehind

Assert what comes before or after the match without including it.

## Lookahead

### Positive Lookahead `(?=...)`

Match only if followed by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "hello" only if followed by " world"
    let re = FuzzyRegex::new("hello(?= world)").unwrap();

    assert!(re.is_match("hello world"));  // Matches "hello"
    assert!(!re.is_match("hello there")); // No match
}
```

### Negative Lookahead `(?!...)`

Match only if NOT followed by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "hello" only if NOT followed by " world"
    let re = FuzzyRegex::new("hello(?! world)").unwrap();

    assert!(re.is_match("hello there")); // Matches "hello"
    assert!(!re.is_match("hello world")); // No match
}
```

## Lookbehind

### Positive Lookbehind `(?<=...)`

Match only if preceded by:

```rust,ignore
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "world" only if preceded by "hello "
    let re = FuzzyRegex::new("(?<=hello )world").unwrap();

    assert!(re.is_match("hello world"));
    assert!(!re.is_match("bye world"));

    // Get match position
    let m = re.find("say hello world here").unwrap();
    assert_eq!(m.start(), 9); // After "hello "
}
```

### Negative Lookbehind `(?<!...)`

Match only if NOT preceded by:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "world" only if NOT preceded by "hello "
    let re = FuzzyRegex::new("(?<!hello )world").unwrap();

    assert!(re.is_match("bye world"));
    assert!(!re.is_match("hello world"));
}
```

## Fuzzy Lookbehind

Lookbehind can include fuzzy matching:

```rust,ignore
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "world" preceded by "hello" with up to 1 error
    let re = FuzzyRegex::new("(?<=(?:hello){e<=1})world").unwrap();

    assert!(re.is_match("hello world")); // Exact
    assert!(re.is_match("hallo world")); // 1 substitution
    assert!(!re.is_match("goodbye world")); // Too different
}
```

### Fuzzy Lookbehind Examples

```rust,ignore
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Allow insertions in lookbehind
    let re = FuzzyRegex::new("(?<=(?:hello){i<=2})world").unwrap();
    assert!(re.is_match("helllo world")); // 2 insertions (ll)
    assert!(re.is_match("hellllo world")); // 3 insertions - no match

    // Allow deletions in lookbehind
    let re = FuzzyRegex::new("(?<=(?:hello){d<=1})world").unwrap();
    assert!(re.is_match("helo world")); // 1 deletion (one 'l')
    assert!(!re.is_match("heo world")); // 2 deletions - no match

    // Combined: substitutions and insertions
    let re = FuzzyRegex::new("(?<=(?:hello){s<=1,i<=1})world").unwrap();
    assert!(re.is_match("hallo world")); // 1 substitution
    assert!(re.is_match("helloh world")); // 1 insertion
    assert!(re.is_match("hallo world")); // matches, prefers lower edits
}
```

### Case Insensitive with Fuzzy

Combine case-insensitive matching with fuzzy for robust matching:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    // Case-insensitive + fuzzy - case differences don't count as edits
    let re = FuzzyRegexBuilder::new("(?i)(?:hello){e<=1}")
        .build()
        .unwrap();

    assert!(re.is_match("hello")); // 0 edits
    assert!(re.is_match("HELLO")); // 0 edits (case difference is free!)
    assert!(re.is_match("hallo")); // 1 edit (actual substitution)

    // Builder's case_insensitive flag works the same way
    let re2 = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .case_insensitive(true)
        .build()
        .unwrap();

    assert!(re2.is_match("HELLO")); // 0 edits
    assert!(re2.is_match("HeLLo")); // 0 edits (all case differences)

    // Get edit count to verify
    let m = re.find("HELLO").unwrap();
    assert_eq!(m.total_edits(), 0); // No edits charged for case!
}
```

### Fuzzy Lookahead

Fuzzy matching also works with lookahead:

```rust,ignore
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "hello" followed by "world" with up to 1 error
    let re = FuzzyRegex::new("hello(?= {e<=1}world)").unwrap();

    assert!(re.is_match("hello world")); // Exact
    assert!(!re.is_match("hello world!")); // ! is not space, lookahead fails
}
```

## Variable-Length Lookbehind

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Variable-length lookbehind (alternation)
    let re = FuzzyRegex::new("(?<=(?:hello|hi) )world").unwrap();
    println!("{}", re.is_match("hi world"));
}
```
