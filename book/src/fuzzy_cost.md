# Cost-Based Matching

Assign different costs to edit operations for fine-grained control.

## Simple Cost Constraint

Use `{c<=N}` to limit total cost (all operations cost 1):

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Total cost ≤ 2 (any combination of edits, each costing 1)
    let re = FuzzyRegex::new("(?:hello){c<=2}").unwrap();

    assert!(re.is_match("hello")); // 0 edits, cost=0
    assert!(re.is_match("hallo")); // 1 sub, cost=1
    assert!(re.is_match("helo"));  // 1 del, cost=1
    assert!(re.is_match("hhello")); // 1 ins, cost=1
    assert!(re.is_match("hallo")); // 1 sub, cost=1
}
```

## Weighted Costs

Assign different costs to different edit types:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Insertions cost 2, others cost 1, total <= 3. Anchored with ^...$ so the
    // cost is measured against the whole string (otherwise a bare "ab" substring
    // would match at cost 0).
    let re = FuzzyRegex::new("^(?:ab){2i+1d+1s+1t<=3}$").unwrap();

    assert!(re.is_match("abc")); // 1 insertion (cost=2)
    assert!(re.is_match("a"));   // 1 deletion (cost=1)
    assert!(re.is_match("ba"));  // 1 transposition (cost=1)
    assert!(!re.is_match("aabc")); // 2 insertions (cost=4) - exceeds limit
}
```

## Cost Syntax

| Syntax | Description |
|--------|-------------|
| `{c<=N}` | Total cost ≤ N |
| `{c<N}` | Total cost < N |
| `{Ni+Md+St+Tt<=N}` | Custom costs for each type |
| `{0i+...}` | Free insertions |
| `{Ni...}` | Insertions cost N |

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // All operations cost 1: {1i+1d+1s+1t<=N} = {c<=N}
    let re1 = FuzzyRegex::new("(?:test){c<=2}").unwrap();

    // Insertions are expensive (typing errors less common)
    let re2 = FuzzyRegex::new("(?:test){3i+1d+1s<=3}").unwrap();

    // Insertions are free (OCR errors)
    let re3 = FuzzyRegex::new("(?:test){0i+1d+1s<=2}").unwrap();
    
    println!("re1: {}", re1.is_match("test"));
    println!("re2: {}", re2.is_match("test"));
    println!("re3: {}", re3.is_match("tset"));
}
```

## Use Cases

- **Typing errors**: Substitutions/deletions more common than insertions
- **OCR errors**: Insertions more common (extra characters)
- **Genetic sequences**: Different scoring matrices
