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

### 1. Use Streaming for Large Data

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

### 2. Use find() for First Match

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
