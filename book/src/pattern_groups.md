# Groups and Alternation

Group patterns and match alternatives.

## Capture Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"(?<user>\w+)@(?<domain>\w+\.\w+)").unwrap();
    let caps = re.captures("john@example.com").unwrap();

    assert_eq!(caps.name("user").unwrap().as_str(), "john");
    assert_eq!(caps.name("domain").unwrap().as_str(), "example.com");
}
```

## Numbered Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    let re = FuzzyRegex::new(r"(\w+)@(\w+)").unwrap();
    let caps = re.captures("test@example").unwrap();

    assert_eq!(caps.get(1).unwrap().as_str(), "test");
    assert_eq!(caps.get(2).unwrap().as_str(), "example");
}
```

## Non-Capturing Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // (?:...) doesn't create a capture group
    let re = FuzzyRegex::new("(?:http|https)://").unwrap();
    println!("{}", re.is_match("http://example.com"));
}
```

## Alternation

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match either option
    let re = FuzzyRegex::new("(foo|bar)").unwrap();

    assert!(re.is_match("foo"));
    assert!(re.is_match("bar"));
    assert!(!re.is_match("baz"));
}
```

## Nested Groups

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;
    let re = FuzzyRegex::new(r"((ab)(cd))").unwrap();
    let caps = re.captures("abcd").unwrap();

    assert_eq!(caps.get(0).unwrap().as_str(), "abcd"); // Full match
    assert_eq!(caps.get(1).unwrap().as_str(), "abcd"); // Outer group
    assert_eq!(caps.get(2).unwrap().as_str(), "ab");   // First inner
    assert_eq!(caps.get(3).unwrap().as_str(), "cd");   // Second inner
}
```

## Fuzzy Groups

Apply fuzziness to groups:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("((?:hello){e<=1})").unwrap();
    println!("{}", re.is_match("helo"));
}
```

## Shared Edit Budgets in Non-Capturing Groups

When a **non-capturing group** has its own fuzziness, the edit budget is **shared** across all pieces inside:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Shared budget: all pieces inside draw from the same {e<=2} pool
    let re = FuzzyRegex::new("(?:(?:hello) (?:world)){e<=2}").unwrap();

    assert!(re.is_match("hello world"));   // 0 edits
    assert!(re.is_match("helo world"));    // 1 deletion (hello side)
    assert!(re.is_match("helo worl"));     // 1 deletion each (2 total, within budget)
    assert!(!re.is_match("hlo wrld"));     // 3 deletions (exceeds budget)
}
```

Per-type limits (`{i<=N,d<=N,s<=N}`) also work with shared budgets:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // At most 1 insertion AND 1 deletion across ALL pieces combined
    let re = FuzzyRegex::new("(?:hello world){i<=1,d<=1}").unwrap();

    assert!(re.is_match("helo world"));   // 1 deletion
    assert!(!re.is_match("helo worl"));   // 2 deletions (exceeds d<=1)
}
```

### How It Works

Without a group budget, each `(?:...){e<=N}` has its own independent edit allowance. Wrapping them in a non-capturing group with fuzziness merges them into a shared pool:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Independent: each piece gets its own {e<=2} → 4 total possible
    let re1 = FuzzyRegex::new("(?:hello){e<=2} (?:world){e<=2}").unwrap();
    assert!(re1.is_match("hlo wrld"));  // 3 deletions across both

    // Shared: both pieces share one {e<=2} budget
    let re2 = FuzzyRegex::new("(?:(?:hello){e<=2} (?:world){e<=2}){e<=2}").unwrap();
    assert!(!re2.is_match("hlo wrld")); // 3 deletions exceeds shared budget
}
```

This is useful for constraining total fuzziness across a multi-word phrase or compound pattern.
