# Performance Tips

Optimize fuzzy-regex for your use case.

## Pattern Design

### 1. Use Specific Edit Limits

```rust
fn main() {
    // Good: Specific limit
    let _ = fuzzy_regex::FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    // Less efficient: Higher limit
    let _ = fuzzy_regex::FuzzyRegex::new("(?:hello){e<=5}").unwrap();
    
    println!("Done");
}
```

Lower edit limits = faster matching.

### 2. Prefer Shorter Patterns

```rust
fn main() {
    // Bitap (fast): ≤64 chars
    let _ = fuzzy_regex::FuzzyRegex::new("(?:short){e<=1}").unwrap();

    // NFA (slower): >64 chars
    let _ = fuzzy_regex::FuzzyRegex::new("(?:very_long_pattern_that_exceeds_sixty_four_characters){e<=1}").unwrap();
    
    println!("Done");
}
```

### 3. Extract Exact Parts

```rust
fn main() {
    // Good: Exact prefix and suffix help prefilter
    let _ = fuzzy_regex::FuzzyRegex::new("exact_prefix (?:fuzzy){e<=1} exact_suffix").unwrap();

    // Slower: Entirely fuzzy
    let _ = fuzzy_regex::FuzzyRegex::new("(?:entirely_fuzzy){e<=1}").unwrap();
    
    println!("Done");
}
```

### 4. Use Greedy Suffix Patterns

```rust
fn main() {
    // Good: .*SUFFIX is optimized with reverse search
    let _ = fuzzy_regex::FuzzyRegex::new(".*test").unwrap();
    let _ = fuzzy_regex::FuzzyRegex::new(".*test~1").unwrap();
    
    // Also works with anchors
    let _ = fuzzy_regex::FuzzyRegex::new("^.*test$").unwrap();
    
    println!("Done");
}
```

Patterns like `.*test` automatically use reverse search to find the suffix first, then match everything before it. This is O(n) instead of O(n²).

### 5. End-Anchor Fuzzy Patterns

```rust
fn main() {
    // Fast: the `$` anchor lets the engine search only near the end
    let _ = fuzzy_regex::FuzzyRegex::new("(?:world){e<=1}$").unwrap();

    // Unanchored: must scan the whole text for candidates
    let _ = fuzzy_regex::FuzzyRegex::new("(?:world){e<=1}").unwrap();

    println!("Done");
}
```

When a fuzzy pattern ends in `$` (single-line, bounded length), both `find`
and `find_iter` search only a small window near the end of the text instead
of scanning every position. A match must finish at the end of the input, so
only the last few bytes can start one.

This makes the cost independent of input size: on a 200 KB input,
`(?:here){e<=1}$` runs in a few microseconds regardless of length, while a
naive full scan grows linearly with the text. The optimization does **not**
apply in multi-line mode (`(?m)`), where `$` matches at every line break.

## Builder Options

### 1. Set Similarity Threshold

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    // Skip low-quality matches early
    let _ = FuzzyRegexBuilder::new("(?:hello){e<=2}")
        .similarity(0.8)
        .build();
    
    println!("Done");
}
```

### 2. Use Case Insensitive at Builder

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    // More efficient than inline (?i)
    let _ = FuzzyRegexBuilder::new("(?:hello)")
        .case_insensitive(true)
        .build();
    
    println!("Done");
}
```

## API Usage

### 1. Use find_iter for Simple Patterns

`find_iter` has specialized fast paths for common pattern types:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Fast path for literal patterns
    let re = FuzzyRegex::new("hello").unwrap();
    let matches: Vec<_> = re.find_iter("hello world hello").collect();
    assert_eq!(matches.len(), 2);
}
```

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Fast path for alternations (uses Aho-Corasick)
    let re = FuzzyRegex::new("(quick|brown|fox)").unwrap();
    let matches: Vec<_> = re.find_iter("the quick brown fox").collect();
    assert_eq!(matches.len(), 3);
}
```

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Fast path for char class + literal patterns
    let re = FuzzyRegex::new(r"\d+ test").unwrap();
    let matches: Vec<_> = re.find_iter("123 test 456 test").collect();
    assert_eq!(matches.len(), 2);
}
```

These fast paths are selected automatically — no configuration needed.

### 2. Use Streaming for Large Data

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    
    // Good: Process in chunks
    let mut stream = re.stream();
    let data = b"hello world";
    for chunk in data.chunks(8) {
        // Process chunk
    }

    // Bad: Load all into memory
    let large_text = "hello world";
    let _matches: Vec<_> = re.find_iter(&large_text).collect();
}
```

### 3. Use find() for First Match

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let text = "hello world";

    // Good: Stop after first match
    if let Some(m) = re.find(text) {
        println!("Found: {}", m.as_str());
    }

    // Unnecessary: Find all when only first needed
    let _all: Vec<_> = re.find_iter(text).collect();
}
```

### 3. Check supports_streaming()

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    
    if re.supports_streaming() {
        // Use streaming API for best performance
        let mut stream = re.stream();
        println!("Streaming supported");
    }
}
```

## Build Configuration

### 1. Release Mode

```bash
cargo build --release
```

### 2. LTO

```toml
[profile.release]
lto = true
codegen-units = 1
```

### 3. SIMD

Enabled by default. Ensure target CPU supports it.

## Common Pitfalls

| Issue | Solution |
|-------|----------|
| Slow with high edits | Lower edit limit |
| High memory usage | Use streaming |
| Slow on long text | Use exact prefix |
| Slow compilation | Enable LTO |

## Pathological Patterns

Some regex patterns can cause O(n²) behavior in naive implementations:

```rust
// Pattern: .*a|b on text of all 'b's
// Each 'b' matches individually
// Naive: O(n) matches × O(n) scan = O(n²)
```

### When It Happens

- **Alternation with wildcards**: `.*a|b`, `(a|b)+`
- **Overlapping matches**: Many ways to match the same text
- **Backtracking patterns**: Complex alternation

### Solution: Hardened Mode

Use `find_all_hardened()` for O(n) guaranteed performance:

```rust,ignore
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new(".*a|b").unwrap();
let text = "bbbbbbbbbbbbbbbb";

// Hardened mode: O(n) guaranteed
let matches = re.find_all_hardened(text);
```

### Performance Comparison

| Text Size | Standard | Hardened |
|-----------|----------|----------|
| 1,000 bytes | 1.08s | 69ms |
| 10,000 bytes | 10.76s | 69ms |

The hardened mode maintains constant time regardless of text size.

### Trade-offs

Hardened mode may be slightly slower for well-behaved patterns (where O(n²) doesn't occur), but it's the safest choice when:

- Pattern behavior is unknown
- Text comes from untrusted sources
- Worst-case performance is critical
