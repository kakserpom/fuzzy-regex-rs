# Performance Tips

Optimize fuzzy-regex for your use case.

## Pattern Design

### 1. Use Specific Edit Limits

```rust
// Good: Specific limit
"(?:hello){e<=1}"

// Less efficient: Higher limit
"(?:hello){e<=5}"
```

Lower edit limits = faster matching.

### 2. Prefer Shorter Patterns

```rust
// Bitap (fast): ≤64 chars
"(?:short){e<=1}"

// NFA (slower): >64 chars
"(?:very_long_pattern_that_exceeds_sixty_four_characters){e<=1}"
```

### 3. Extract Exact Parts

```rust
// Good: Exact prefix and suffix help prefilter
"exact_prefix (?:fuzzy){e<=1} exact_suffix"

// Slower: Entirely fuzzy
"(?:entirely_fuzzy){e<=1}"
```

## Builder Options

### 1. Set Similarity Threshold

```rust
// Skip low-quality matches early
FuzzyRegexBuilder::new("(?:hello){e<=2}")
    .similarity(0.8)
    .build();
```

### 2. Use Case Insensitive at Builder

```rust
// More efficient than inline (?i)
FuzzyRegexBuilder::new("(?:hello)")
    .case_insensitive(true)
    .build();
```

## API Usage

### 1. Use Streaming for Large Data

```rust
// Good: Process in chunks
let mut stream = re.stream();
for chunk in data.chunks(8192) {
    // Process chunk
}

// Bad: Load all into memory
let matches: Vec<_> = re.find_iter(&large_text).collect();
```

### 2. Use find() for First Match

```rust
// Good: Stop after first match
if let Some(m) = re.find(text) {
    // ...
}

// Unnecessary: Find all when only first needed
let all: Vec<_> = re.find_iter(text).collect();
```

### 3. Check supports_streaming()

```rust
if re.supports_streaming() {
    // Use streaming API for best performance
    let mut stream = re.stream();
    // ...
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
