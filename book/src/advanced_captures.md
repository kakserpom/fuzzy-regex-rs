# Capture Groups

Extract specific portions of matches.

## Named Capture Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"(?<user>\w+)@(?<domain>\w+\.\w+)").unwrap();
    let caps = re.captures("john@example.com").unwrap();

    assert_eq!(caps.name("user").unwrap().as_str(), "john");
    assert_eq!(caps.name("domain").unwrap().as_str(), "example.com");
}
```

## Numbered Capture Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    let re = FuzzyRegex::new(r"(\w+)@(\w+)").unwrap();
    let caps = re.captures("test@example").unwrap();

    assert_eq!(caps.get(1).unwrap().as_str(), "test");
    assert_eq!(caps.get(2).unwrap().as_str(), "example");
    assert_eq!(caps.get(0).unwrap().as_str(), "test@example"); // Full match
}
```

## Nested Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    let re = FuzzyRegex::new(r"((?P<outer>\w+)-(?P<inner>\w+))").unwrap();
    let caps = re.captures("hello-world").unwrap();

    assert_eq!(caps.get(0).unwrap().as_str(), "hello-world");
    assert_eq!(caps.get(1).unwrap().as_str(), "hello-world");
    assert_eq!(caps.name("outer").unwrap().as_str(), "hello");
    assert_eq!(caps.name("inner").unwrap().as_str(), "world");
}
```

## Capture Groups with Fuzzy Matching

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"(?<word>\w+){e<=1}").unwrap();
    let caps = re.captures("tset").unwrap();

    assert_eq!(caps.name("word").unwrap().as_str(), "tset");
}
```

### Fuzzy Edits Information

When using fuzzy matching, you can get detailed edit information:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"(?:hello){e<=2}").unwrap();
    let m = re.find("helllo").unwrap();

    // Get total edits
    println!("Total edits: {}", m.total_edits()); // 1 (1 insertion)

    // Get detailed counts: (insertions, deletions, substitutions)
    let (ins, del, sub) = m.fuzzy_counts();
    println!("Insertions: {}, Deletions: {}, Substitutions: {}", ins, del, sub);

    // Get positions of changes
    let (ins_pos, del_pos, sub_pos) = m.fuzzy_changes();
    println!("Insertion positions: {:?}", ins_pos);
}
```

### Handler Overrides in Captures

When using handlers with `MatchOverride`, the overrides are tracked in captures:

```rust
fn main() {
    use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

    let re = FuzzyRegexBuilder::new(r"(prefix(?call:handler)suffix)")
        .handler("handler", |text, pos| {
            if text[pos..].starts_with("XYZ") {
                HandlerResult::MatchOverride(3, "xyz".to_string())
            } else {
                HandlerResult::NoMatch
            }
        })
        .build()
        .unwrap();

    let caps = re.captures("prefixXYZsuffix").unwrap();

    // The captured text shows the override
    assert_eq!(caps.get(1).unwrap().as_str(), "prefixxyzsuffix");

    // Handler overrides track (start_byte, end_byte, override_text)
    let overrides = caps.handler_overrides();
    println!("Handler overrides: {:?}", overrides);
    // Output: [(6, 9, "xyz")]
}
```

### Multiple Capture Groups with Fuzzy

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Two fuzzy groups with different limits
    let re = FuzzyRegex::new(r"(?<a>\w+){e<=1} (?<b>\w+){e<=1}").unwrap();
    let caps = re.captures("helo wrold").unwrap();

    assert_eq!(caps.name("a").unwrap().as_str(), "helo"); // 1 substitution
    assert_eq!(caps.name("b").unwrap().as_str(), "wrold"); // 1 substitution

    // Each group tracks its own edits
    // Note: Fuzzy matching applies to the overall pattern
}
```

## Iterating Captures

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    let re = FuzzyRegex::new(r"(\w+)").unwrap();
    let caps = re.captures("hello world").unwrap();

    for cap in caps.iter() {
        if let Some(m) = cap {
            println!("Group: '{}'", m.as_str());
        }
    }
}
```

## Accessing All Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    let re = FuzzyRegex::new(r"(\w+)@(\w+)").unwrap();
    let caps = re.captures("a@b").unwrap();

    assert_eq!(caps.len(), 3); // 0 (full) + 2 groups

    // Access each group by index
    for i in 0..caps.len() {
        if let Some(m) = caps.get(i) {
            println!("Group {}: {}", i, m.as_str());
        }
    }
}
```
