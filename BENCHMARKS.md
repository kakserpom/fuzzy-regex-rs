# fuzzy-regex Benchmarks

## Quick Start

Run benchmarks from the `examples` directory:

```bash
# Basic performance benchmarks
cargo run --release --example quick_bench
cargo run --release --example bench_vs_mrab

# Streaming benchmarks
cargo run --release --example bench_mbps

# Unicode benchmarks
cargo run --release --example bench_unicode
```

## Benchmark Results (MacBook Pro M1 Max)

### Quick Bench (`quick_bench`)

**Compilation:**

| Pattern                                             | Time         |
|-----------------------------------------------------|--------------|
| Simple pattern `(?:hello){e<=2}`                    | 2.04 μs/iter |
| Complex pattern `(?:hello){i<=1,d<=1,s<=2,1i+1d<3}` | 2.41 μs/iter |

**Short text (44 bytes):**

| Operation               | Time         |
|-------------------------|--------------|
| Exact match             | 0.10 μs/iter |
| Fuzzy 1 edit            | 2.12 μs/iter |
| Fuzzy 2 edits           | 3.01 μs/iter |
| Substitution constraint | 2.58 μs/iter |
| Cost constraint         | 6.93 μs/iter |

**Long text (33,400 bytes):**
| Operation | Time |
|-----------|------|
| Fuzzy 2 edits (find first) | 12.44 μs/iter |
| Fuzzy 2 edits (find_iter count) | 1,939 μs/iter |
| Fuzzy 1 edit (find_iter count) | 559 μs/iter |
| No match (full scan) | 256 μs/iter |

**Edit distance scaling:**
| Edits | Time |
|-------|------|
| 0 (exact) | 1.13 μs/iter |
| 1 | 11.15 μs/iter |
| 2 | 12.42 μs/iter |
| 3 | 13.60 μs/iter |

---

### Streaming Benchmarks (`bench_mbps`)

Pattern: `transportation` with transposition `transporattion` at end of text.

**Throughput by text size (k=2):**
| Text Size | Throughput |
|-----------|------------|
| 114 bytes | 6.9 MB/s |
| 2 KB | 75.7 MB/s |
| 20 KB | 160.7 MB/s |
| 200 KB | 184.6 MB/s |

**By k-value (20KB text):**
| k | Throughput |
|---|------------|
| 1 | 201 MB/s |
| 2 | 120 MB/s |
| 3 | 69 MB/s |

**No-match case (200KB):** 186 MB/s

**Transposition overhead:** ~1%

---

### vs mrab Bench (`bench_vs_mrab`)

| Test Case                      | Time         |
|--------------------------------|--------------|
| Short text (44B), fuzzy e<=1   | 0.06 μs/iter |
| Medium text (191B), fuzzy e<=2 | 0.06 μs/iter |
| Long text (3.8KB), fuzzy e<=2  | 0.06 μs/iter |
| Substitution constraint        | 0.06 μs/iter |
| No match (short)               | 0.38 μs/iter |
| No match (medium)              | 1.53 μs/iter |
| DNA sequence (1KB)             | 0.09 μs/iter |

---

### Unicode Benchmarks (`bench_unicode`)

**Compilation:**

| Pattern                        | Time         |
|--------------------------------|--------------|
| ASCII pattern (no unicode)     | 1.97 μs/iter |
| ASCII pattern (unicode mode)   | 2.10 μs/iter |
| Unicode pattern (unicode mode) | 3.22 μs/iter |

**Short ASCII text (44 bytes):**

| Operation          | Time         |
|--------------------|--------------|
| ASCII exact match  | 0.10 μs/iter |
| ASCII fuzzy 1 edit | 1.55 μs/iter |

**Short Unicode text (60 bytes):**

| Operation            | Time         |
|----------------------|--------------|
| Unicode exact match  | 0.06 μs/iter |
| Unicode fuzzy 1 edit | 0.04 μs/iter |
| Unicode substitution | 0.05 μs/iter |

**Unicode character classes:**

| Pattern                      | Time         |
|------------------------------|--------------|
| ASCII `\w+` (no unicode)     | 0.06 μs/iter |
| Unicode `\w+` (unicode mode) | 0.14 μs/iter |
| ASCII `\d+` (no unicode)     | 0.03 μs/iter |
| Unicode `\d+` (unicode mode) | 0.05 μs/iter |

**Cyrillic fuzzy matching:**

| Pattern                | Time         |
|------------------------|--------------|
| Cyrillic fuzzy 1 edit  | 0.11 μs/iter |
| Cyrillic fuzzy 2 edits | 0.12 μs/iter |

---

## Running All Benchmarks

```bash
# Clone the repo
git clone https://github.com/kakserpom/fuzzy-regex
cd fuzzy-regex

# Build in release mode
cargo build --release

# Run quick benchmarks
cp benchmarks/quick_bench.rs examples/
cargo run --release --example quick_bench

# Run comparison benchmarks
cp benchmarks/bench_vs_mrab.rs examples/
cargo run --release --example bench_vs_mrab

# Run streaming benchmarks
cargo run --release --example bench_mbps

# Run unicode benchmarks
cp benchmarks/bench_unicode.rs examples/
cargo run --release --example bench_unicode
```

## Notes

- All benchmarks use `cargo build --release` for optimized builds
- LTO and single codegen unit enabled in release profile
- SIMD is enabled by default (`default = ["simd"]`)

---

## Pathological Pattern Benchmark

Pathological patterns like `.*a|b` on text of 'b's can cause O(n²) behavior. This benchmark compares three all-matches algorithms.

Run with:
```bash
cargo test --lib benchmark_pathological_pattern -- --nocapture
```

### Results (Pattern `.*a|b` on text of 'b's)

| Text Size | find_all | two_pass | hardened | Speedup |
|-----------|----------|----------|----------|---------|
| 1,000 bytes | 1.08s | 1.10s | **69ms** | 15x |
| 5,000 bytes | 5.47s | 5.43s | **69ms** | 79x |
| 10,000 bytes | 10.76s | 10.85s | **69ms** | **156x** |

### Complexity

| Algorithm | Complexity | Notes |
|-----------|------------|-------|
| find_all | O(n²) | Standard "find, advance, repeat" |
| two_pass | O(n²) | Pass 1 is fast, but Pass 2 still verifies each match |
| hardened | O(n) | Tracks all DFA states simultaneously |

### Why Hardened Mode is Faster

The hardened mode processes each character exactly once by tracking all active DFA states simultaneously. For the pattern `.*a|b` on "bbbb":

1. Start at position 0 with state (start, pos=0)
2. Process 'b': State can continue with itself (accepting state)
3. When we hit a character that can't continue (or end of text), emit match
4. Move to next unmatched position and repeat

This avoids the O(n) re-scanning that happens in naive implementations.
