# FuzzyRegexBuilder

Builder for customizing regex construction.

## Basic Usage

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello)")
        .build()
        .unwrap();
    
    println!("Created");
}
```

## Builder Options

### Similarity Threshold

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=2}")
        .similarity(0.8)  // Minimum similarity 0.0-1.0
        .build()
        .unwrap();
    
    println!("Created");
}
```

### Case Insensitivity

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello)")
        .case_insensitive(true)
        .build()
        .unwrap();

    assert!(re.is_match("HELLO"));
    assert!(re.is_match("Hello"));
}
```

### Multi-line Mode

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("^hello$")
        .multi_line(true)  // ^ and $ match line boundaries
        .build()
        .unwrap();
    
    println!("Created");
}
```

### Dot-all Mode

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("a.b")
        .dot_all(true)  // . matches newlines
        .build()
        .unwrap();
    
    println!("Created");
}
```

### Partial Matching

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .partial(true)  // Matches at end of text are partial
        .build()
        .unwrap();

    let m = re.find("hello").unwrap();
    assert!(m.partial()); // Match reaches end of text
}
```

### Timeout

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;
    use std::time::Duration;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=5}")
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    
    println!("Created");
}
```

### Match End Policy

When several end positions are valid within the edit budget, `match_end_policy`
chooses which one is reported. The default, `LongestWithinBudget`, reports the
widest span the budget allows; `MinEdit` reports the tightest alignment (fewest
edits, then closest to the pattern length, then shortest span), matching
mrab-regex's minimal-error reporting for large or unlimited `{e}` budgets.

```rust
fn main() {
    use fuzzy_regex::{FuzzyRegexBuilder, MatchEndPolicy};

    // Unlimited budget: many ends are valid for "(?:error){e}" in "regex failure".
    let default = FuzzyRegexBuilder::new(r"(?:error){e}").build().unwrap();
    assert_eq!(
        default.find("regex failure").map(|m| (m.start(), m.end())),
        Some((0, 8)) // widest span within budget
    );

    let min = FuzzyRegexBuilder::new(r"(?:error){e}")
        .match_end_policy(MatchEndPolicy::MinEdit)
        .build()
        .unwrap();
    assert_eq!(
        min.find("regex failure").map(|m| (m.start(), m.end())),
        Some((0, 5)) // tightest alignment: "regex"
    );
}
```

`find`, `find_iter`, and `captures` all agree under the chosen policy.

### Word List Aho-Corasick Threshold

For large `\L<name>` word lists, fuzzy-regex can match with an Aho-Corasick
automaton instead of an NFA alternation (see [Word Lists](advanced_wordlists.md)).
`word_list_ac_threshold` sets the minimum list size that triggers the automaton;
smaller lists use the NFA. It never changes results, only performance. Defaults
to `DEFAULT_WORD_LIST_AC_THRESHOLD` (64). Only relevant with the `word-list-ac`
feature (enabled by default).

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let mut re = FuzzyRegexBuilder::new(r"\b\L<w>\b")
        .word_list_ac_threshold(1000)
        .build()
        .unwrap();
    re.set_word_list("w", vec!["cat", "dog"]);
    assert!(re.is_match("cat"));
}
```

## Chaining Options

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
        .case_insensitive(true)
        .similarity(0.7)
        .partial(true)
        .multi_line(true)
        .build()
        .unwrap();
    
    println!("Created");
}
```
