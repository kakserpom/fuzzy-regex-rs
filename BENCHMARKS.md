# fuzzy-regex Benchmarks

## Quick Start

Run benchmarks from the `examples` directory:

```bash
# Basic performance benchmarks
cargo run --release --example quick_bench
cargo run --release --example bench_vs_mrab

# Streaming benchmarks
cargo run --release --example bench_mbps
```

## Benchmark Results (MacBook Pro M1 Max)

### Quick Bench (`quick_bench`)

**Compilation:**
| Pattern | Time |
|---------|------|
| Simple pattern `(?:hello){e<=2}` | 1.98 μs/iter |
| Complex pattern `(?:hello){i<=1,d<=1,s<=2,1i+1d<3}` | 2.60 μs/iter |

**Short text (44 bytes):**
| Operation | Time |
|-----------|------|
| Exact match | 0.09 μs/iter |
| Fuzzy 1 edit | 2.00 μs/iter |
| Fuzzy 2 edits | 2.90 μs/iter |

**Long text (33,400 bytes):**
| Operation | Time |
|-----------|------|
| Fuzzy 2 edits (find first) | 12.99 μs/iter |
| Fuzzy 2 edits (find_iter count) | 1,907 μs/iter |
| Fuzzy 1 edit (find_iter count) | 570 μs/iter |
| No match (full scan) | 259 μs/iter |

**Edit distance scaling:**
| Edits | Time |
|-------|------|
| 0 (exact) | 1.25 μs/iter |
| 1 | 10.61 μs/iter |
| 2 | 12.38 μs/iter |
| 3 | 13.95 μs/iter |

---

### Streaming Benchmarks (`bench_mbps`)

Pattern: `transportation` with transposition `transporattion` at end of text.

| Text Size | Throughput | Throughput |
|-----------|------------|------------|
| 100 bytes | 52 Mbps    | 6.5 MB/s   |
| 1 KB      | 377 Mbps   | 47 MB/s    |
| 10 KB     | 1,135 Mbps | 142 MB/s   |
| 100 KB    | 1,429 Mbps | 179 MB/s   |
| 200 KB    | 1,464 Mbps | 183 MB/s   |
| 1 MB      | 1,446 Mbps | 181 MB/s   |

**By k-value (200KB text):**
| k | Throughput | Throughput |
|---|------------|-------------|
| 1 | 1,984 Mbps | 248 MB/s |
| 2 | 1,430 Mbps | 179 MB/s |
| 3 | 711 Mbps | 89 MB/s |

**No-match case (200KB):** 1,486 Mbps (185 MB/s)

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
```

## Notes

- All benchmarks use `cargo build --release` for optimized builds
- LTO and single codegen unit enabled in release profile
- SIMD is enabled by default (`default = ["simd"]`)
