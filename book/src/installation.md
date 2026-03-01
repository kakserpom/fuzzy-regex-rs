# Installation

Add `fuzzy-regex` to your `Cargo.toml`:

```toml
[dependencies]
fuzzy-regex = "0.1"
```

## Feature Flags

- `simd` (default): Enable SIMD optimizations for faster matching on x86_64 and ARM
- `mimalloc`: Use mimalloc allocator for better performance in memory-intensive workloads

```toml
[dependencies]
fuzzy-regex = { version = "0.1", default-features = false }
# Or with mimalloc
fuzzy-regex = { version = "0.1", features = ["simd", "mimalloc"] }
```

## MSRV

The minimum supported Rust version is **1.88**.

## Quick Verification

```rust
use fuzzy_regex::FuzzyRegex;

fn main() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    println!("{}", re.is_match("hallo")); // true
}
```
