# Fuzziness Markers

Detailed syntax for controlling fuzzy matching.

## Basic Markers

| Marker | Description | Example |
|--------|-------------|---------|
| `{e<=N}` | Total edits ≤ N | `{e<=2}` |
| `{i<=N}` | Insertions ≤ N | `{i<=1}` |
| `{d<=N}` | Deletions ≤ N | `{d<=1}` |
| `{s<=N}` | Substitutions ≤ N | `{s<=1}` |
| `{t<=N}` | Transpositions ≤ N | `{t<=1}` |

## Combining Markers

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Allow 1 insertion AND 1 deletion
    let re1 = FuzzyRegex::new("(?:hello){i<=1,d<=1}").unwrap();

    // Allow up to 2 substitutions OR up to 1 deletion (combined constraint)
    let re2 = FuzzyRegex::new("(?:hello){s<=2,d<=1}").unwrap();

    // Each constraint is independent
    // The match must satisfy ALL specified constraints
    println!("re1 matches 'helo': {}", re1.is_match("helo"));
    println!("re2 matches 'hallo': {}", re2.is_match("hallo"));
}
```

## Character Class Restrictions

Restrict which characters can be used for edits:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Only allow substitutions with lowercase letters
    let re1 = FuzzyRegex::new(r"(?:hello){s<=1:[a-z]}").unwrap();
    assert!(re1.is_match("hallo"));  // 'a' is in [a-z]
    assert!(!re1.is_match("h3llo")); // '3' is not in [a-z]

    // Only allow insertions of digits
    let re2 = FuzzyRegex::new(r"(?:hello){i<=1:[0-9]}").unwrap();

    // Only allow substitutions with whitespace
    let re3 = FuzzyRegex::new(r"(?:hello){s<=1:\s}").unwrap();
}
```

## Min/Max Error Ranges

```rust,ignore
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Require at least 1 edit, allow up to 2
    let re = FuzzyRegex::new("(?:hello){e>=1,e<=2}").unwrap();

    // Minimum errors with shorthand
    let re2 = FuzzyRegex::new("(?:hello){1e<=2}").unwrap();
    
    println!("re matches 'hello': {}", re.is_match("hello"));
    println!("re matches 'hallo': {}", re.is_match("hallo"));
}
```

## Editing Classes

Apply fuzziness to specific character classes:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Apply different limits to different parts. Each fuzzy piece must be a
    // group, so the second word is written `(?:world){e<=1}`.
    let re = FuzzyRegex::new("(?:hello){e<=1} (?:world){e<=1}").unwrap();
    assert!(re.is_match("helo worled")); // 1 edit in each piece
}
```

## Shared Group Budget

When a **non-capturing group** has its own fuzziness, the edit budget is shared across all pieces inside it:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Shared budget: total edits across "hello" and "world" combined <= 2
    let re = FuzzyRegex::new("(?:hello world){e<=2}").unwrap();
    // "hello" and "world" draw from the same pool of 2 edits

    assert!(re.is_match("hello world"));  // 0 edits
    assert!(re.is_match("helo world"));   // 1 deletion, 1 remaining
    assert!(re.is_match("helo worl"));    // 1 deletion each = 2 total, within budget
    assert!(!re.is_match("hlo wrld"));    // 3 deletions, exceeds budget

    // Per-type limits also shared across the group
    let re2 = FuzzyRegex::new("(?:hello world){i<=1,d<=1}").unwrap();
    // Maximum 1 insertion AND 1 deletion across all pieces

    assert!(re2.is_match("helo world")); // 1 deletion
    assert!(!re2.is_match("helo worl")); // 2 deletions, exceeds d<=1
}
```

This ensures that one piece doesn't exhaust the shared budget, giving predictable results for multi-segment fuzzy patterns.
