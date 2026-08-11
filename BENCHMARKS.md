# fuzzy-regex Benchmarks

Performance and benchmark documentation for fuzzy-regex: comparison vs Rust's
standard `regex` crate and Python `mrab-regex`, streaming/unicode results, and
the performance investigations and optimizations applied.

## Benchmark Configuration

All benchmarks live in `benches/` and run in release mode. Unless a section
says otherwise:

- Platform: MacBook Pro M1 Max
- Rust: 1.93+
- Optimization: Release mode with LTO and a single codegen unit
- SIMD enabled by default (`default = ["simd"]`)
- The comprehensive suite is `benches/bench_suite.rs`
  (`cargo bench --bench bench_suite`)
- `vs regex` tables use `is_match` (first match) on short (48 B), long
  (4800 B), and very long (48000 B) copies of
  `"The quick brown fox jumps over the lazy dog! 123"`
- Timing uses automatic calibration to ~50 ms per measurement

## Benchmark Results

### vs `regex` crate

#### Short Text (48 bytes)

| Pattern                               | fuzzy-regex | regex crate | Winner         |
|---------------------------------------|------------:|------------:|----------------|
| exact literal                         | 24.9 ns     | 9.0 ns      | regex 2.8x     |
| no match                              | 45.5 ns     | 9.4 ns      | regex 4.8x     |
| optional char `qu?ick`                | 57.8 ns     | 21.7 ns     | regex 2.7x     |
| one-or-more `qu+ick`                  | 57.7 ns     | 24.2 ns     | regex 2.4x     |
| zero-or-more `qu*ick`                 | 57.8 ns     | 21.7 ns     | regex 2.7x     |
| start anchor `^The`                   | 41.3 ns     | 11.9 ns     | regex 3.5x     |
| end anchor `dog$`                     | 31.5 ns     | 10.5 ns     | regex 3x       |
| lowercase class `[a-z]+`              | 53.4 ns     | 11.5 ns     | regex 4.6x     |
| digit class `[0-9]+`                  | 89.6 ns     | 24.6 ns     | regex 3.6x     |
| digits `\d+`                          | 101.1 ns    | 57.2 ns     | regex 1.8x     |
| word chars `\w+`                      | 34.9 ns     | 11.3 ns     | regex 3.1x     |
| whitespace `\s+`                      | 38.5 ns     | 12.6 ns     | regex 3.1x     |
| non-digits `\D+`                      | 82.5 ns     | 11.2 ns     | regex 7.4x     |
| word boundary `\b\w+\b`               | 14.8 ns     | 11.9 ns     | regex 1.2x     |
| 4-char word `\b\w{4}\b`               | 49.2 ns     | 39.3 ns     | regex 1.3x     |
| exactly 3 digits `\d{3}`              | 90.1 ns     | 59.2 ns     | regex 1.5x     |
| lazy digits `\d+?`                    | 74.9 ns     | 57.6 ns     | regex 1.3x     |
| alternation `(?:quick\| brown\| fox)` | 59.6 ns     | 14.5 ns     | regex 4.1x     |
| wildcard `quick.*fox`                 | 41.9 ns     | 37.9 ns     | tie             |
| repetition `(?:quick){2}`             | 45.5 ns     | 10.9 ns     | regex 4.2x     |
| decimal `\d+\.\d+`                    | 50.2 ns     | 9.9 ns      | regex 5.1x     |

#### Long Text (4800 bytes)

| Pattern                   | fuzzy-regex | regex crate | Winner          |
|---------------------------|------------:|------------:|-----------------|
| exact literal             | 38.7 ns     | 9.3 ns      | regex 4.2x      |
| digits `\d+`              | 104.5 ns    | 58.5 ns     | regex 1.8x      |
| char class `[a-z]+`       | 55.0 ns     | 11.8 ns     | regex 4.7x      |
| repetition `(?:quick){2}` | 326.5 ns    | 307.8 ns    | tie             |
| wildcard `quick.*fox`     | 34.1 ns     | 35.0 ns     | tie             |
| word boundary `\b\w+\b`   | 15.1 ns     | 12.2 ns     | regex 1.2x      |

#### Very Long Text (48000 bytes)

| Pattern                   | fuzzy-regex | regex crate | Winner           |
|---------------------------|------------:|------------:|------------------|
| exact literal             | 38.7 ns     | 9.3 ns      | regex 4.2x       |
| digits `\d+`              | 104.1 ns    | 58.8 ns     | regex 1.8x       |
| char class `[a-z]+`       | 54.5 ns     | 11.9 ns     | regex 4.6x       |
| repetition `(?:quick){2}` | 3003.9 ns   | 2922.5 ns   | tie              |
| wildcard `quick.*fox`     | 32.9 ns     | 35.5 ns     | tie              |
| word boundary `\b\w+\b`   | 15.0 ns     | 12.1 ns     | regex 1.2x       |

### Key Wins

fuzzy-regex is competitive with the `regex` crate for:

1. **Repetition (`(?:x){2}`)**: parity on long/very-long text (the engine now
   matches the regex crate instead of a 5-6x loss).
2. **Word boundaries (`\b\w+\b`)**: within 1.2x of the regex crate on all text
   lengths.
3. **Digit patterns (`\d+`)**: within 1.8x, better than most other classes.

The strongest advantages are outside plain `is_match`:

4. **Streaming search**: 132 MB/s on a 200 KB text (no-match case).
5. **Pathological patterns**: hardened mode is O(n) — 356x faster than the
   naive all-matches scan at 10 KB.
6. **vs Python mrab-regex**: see `docs/OPT_FUZZ_FINDINGS.md`.

### Areas for Improvement

1. **Wildcard `quick.*fox`**: resolved — `find`/`is_match` now use a dedicated
   two-literal-search fast path (`PrefixDotStarSuffix` shape detection at
   compile time; memmem prefix scan + forward/`rfind` suffix instead of the
   DFA's full-text scan). `is_match` short-circuits to a bounded existence
   check. Results: 41.9 ns / 34.1 ns / 32.9 ns on 48 B / 4.8 KB / 48 KB —
   parity with the regex crate on all three sizes (was 2067 ns / 205 µs /
   2.07 ms before the first fix, then 243 ns / 21 µs / 209 µs after routing
   through `dfa.find`).
2. **Exact literal matching**: 3-4x slower than regex crate (SIMD memchr).
3. **Character classes**: 3-7x slower (prefilter not as selective).
4. **Alternation**: 4x slower on short text.
5. **Cost-constraint patterns** (`{1i+1d<3}`): ~72 µs on short text — see
   Quick Bench below.

### Quick Bench (`quick_bench`)

`cargo bench --bench quick_bench`

**Compilation:**

| Pattern                                             | Time         |
|-----------------------------------------------------|--------------|
| Simple pattern `(?:hello){e<=2}`                    | 3.42 μs/iter |
| Complex pattern `(?:hello){i<=1,d<=1,s<=2,1i+1d<3}` | 2.65 μs/iter |

**Short text (44 bytes):**

| Operation               | Time          |
|-------------------------|---------------|
| Exact match             | 0.01 μs/iter  |
| Fuzzy 1 edit            | 1.88 μs/iter  |
| Fuzzy 2 edits           | 3.37 μs/iter  |
| Substitution constraint | 8.57 μs/iter  |
| Cost constraint         | 71.57 μs/iter |

**Long text (33,400 bytes):**

| Operation | Time           |
|-----------|----------------|
| Fuzzy 2 edits (find first) | 4.97 μs/iter |
| Fuzzy 2 edits (find_iter count) | 2,008 μs/iter |
| Fuzzy 1 edit (find_iter count) | 647 μs/iter |
| No match (full scan) | 272 μs/iter |

**Edit distance scaling (long text):**

| Edits | Time        |
|-------|-------------|
| 0 (exact) | 1.57 μs/iter |
| 1 | 2.58 μs/iter |
| 2 | 4.96 μs/iter |
| 3 | 6.88 μs/iter |

**DNA sequence (10,000 bp):** `ACGTACGT` with `{e<=2}` → 0.57 μs/iter.

### Streaming Benchmarks (`bench_suite`)

Pattern: `transportation` with transposition `transporattion` at end of text,
`BitapMatcher::find_first_streaming`.

**Throughput by text size (k=2):**

| Text Size | Throughput |
|-----------|------------|
| 114 bytes | 4.1 MB/s |
| 2 KB | 47.5 MB/s |
| 20 KB | 113.8 MB/s |
| 200 KB | 132.0 MB/s |

**By k-value (20 KB text):**

| k | Throughput |
|---|------------|
| 1 | 148.0 MB/s |
| 2 | 114.7 MB/s |
| 3 | 79.4 MB/s |

**No-match case (138 KB):** 133.4 MB/s

### vs mrab Bench (`bench_vs_mrab`)

`cargo bench --bench bench_vs_mrab` — fuzzy-regex side of the comparison (the
Python side and combined table come from `benches/mrab_compare.sh`).

| Test Case                      | Time         |
|--------------------------------|--------------|
| Short text (44B), fuzzy e<=1   | 1.32 μs/iter |
| Medium text (191B), fuzzy e<=2 | 0.67 μs/iter |
| Long text (3.8KB), fuzzy e<=2  | 0.65 μs/iter |
| Substitution constraint        | 5.97 μs/iter |
| No match (short)               | 0.69 μs/iter |
| No match (medium)              | 1.91 μs/iter |
| DNA sequence (1KB)             | 0.57 μs/iter |

### Unicode Benchmarks (`bench_unicode`)

`cargo bench --bench bench_unicode`

**Compilation:**

| Pattern                        | Time         |
|--------------------------------|--------------|
| ASCII pattern (no unicode)     | 3.48 μs/iter |
| ASCII pattern (unicode mode)   | 3.46 μs/iter |
| Unicode pattern (unicode mode) | 3.87 μs/iter |

**Short ASCII text (44 bytes):**

| Operation          | Time         |
|--------------------|--------------|
| ASCII exact match  | 0.01 μs/iter |
| ASCII fuzzy 1 edit | 1.90 μs/iter |

**Short Unicode text (60 bytes):**

| Operation            | Time         |
|----------------------|--------------|
| Unicode exact match  | 0.02 μs/iter |
| Unicode fuzzy 1 edit | 0.06 μs/iter |
| Unicode substitution | 0.06 μs/iter |

**Unicode character classes:**

| Pattern                      | Time         |
|------------------------------|--------------|
| ASCII `\w+` (no unicode)     | 0.04 μs/iter |
| Unicode `\w+` (unicode mode) | 0.15 μs/iter |
| ASCII `\d+` (no unicode)     | 0.03 μs/iter |
| Unicode `\d+` (unicode mode) | 0.06 μs/iter |

**Long Unicode text (6 KB):** Unicode fuzzy `find` 1.54 μs/iter, `find_iter`
count 111 μs/iter.

**Cyrillic fuzzy matching:**

| Pattern                | Time         |
|------------------------|--------------|
| Cyrillic fuzzy 1 edit  | 1.31 μs/iter |
| Cyrillic fuzzy 2 edits | 1.57 μs/iter |

## Optimization Techniques Used

1. **Compile-time strategy caching**: All NFA pattern checks are done once at construction
2. **Memchr fast path**: Direct byte search for simple literals
3. **Aho-Corasick caching**: Automaton built once during construction
4. **Character class plus detection**: Direct byte scanning for `\d+`, `\w+`, `\s+`
5. **Word boundary optimization**: Fast word-edge detection
6. **NEON SIMD for ARM**: Vectorized character class and Teddy search on Apple Silicon
7. **Two-pass algorithm**: Reverse prefilter + forward verification for all-matches
8. **Hardened mode**: True O(n) for pathological patterns
9. **End-anchor windowing**: Fuzzy patterns ending in `$` (single-line) search only a bounded window near the end of the text and shift results back, making `find`/`find_iter` cost independent of input size instead of a full O(n) scan
10. **`LITERAL.*LITERAL` two-literal search**: compile-time NFA shape detection (`PrefixDotStarSuffix`) for `LITERAL .* LITERAL` / `LITERAL .+ LITERAL` (greedy or lazy middle); `find` locates the boundary literals with memmem (prefix scan + rightmost/leftmost suffix, line-terminator aware) instead of the DFA's full-text scan, and `is_match` short-circuits to a bounded existence check

## Performance Optimization Findings

The performance investigations and optimizations attempted for fuzzy-regex.

### Current Performance

From `mini_bench` (`cargo bench --bench mini_bench`):

```
short text, 1 edit:         2.1 µs
short text, 2 edits:        3.4 µs
long text, 2 edits:        47.1 µs
long text, no match:       36.9 µs
```

### Optimizations Applied

#### 1. Dependencies Removed
- `smallvec` → replaced with `Vec`
- `smartstring` → replaced with `String`
- `smartcow` → replaced with `std::borrow::Cow`

#### 2. GuardNFA ASCII Optimization
Added byte-level path for ASCII text, avoiding `Vec<char>` allocation.
- ~27% improvement for exact matches on ASCII text

#### 3. Alternation Fast Path
Added direct path in `FuzzyRegex::find()` for exact alternations (e.g., "cat|dog|bat").
- Avoids expensive `find_iter -> find_all` path
- ~25x improvement (16µs -> 0.6µs)

#### 4. Pigeonhole Prefilter Extension
Extended pigeonhole prefilter to work with shorter patterns.
- Changed threshold from 10+ chars to `2*(k+1)` chars
- For k=1: now uses pigeonhole for patterns >= 4 chars
- For k=2: now uses pigeonhole for patterns >= 6 chars
- ~38% improvement for long text fuzzy matching

#### 5. SIMD Batch Parallel
Use batch parallel SIMD search when prefilter selectivity <= 2.
- Uses lazy search otherwise (better for early termination)

### Investigations That Didn't Help

1. **EditCounts Copy** - Caused ~2x regression (reverted)
2. **Option<Vec>** - Added branch overhead (reverted)
3. **Mimalloc allocator** - Added overhead (reverted)
4. **SmartString** - Slower than String for most cases

### Long Text Fuzzy Matching Analysis

Long text fuzzy matching is fundamentally expensive:

- For pattern "lorem" with e<=1 in 4KB text:
  - Fuzzy prefilter finds ~500 candidate positions
  - Bitap runs ~70ns per candidate
  - Total: 500 * 70ns = 35µs (matches observed)

- Performance varies by match position:
  - Match at start: ~1.5µs (fast termination)
  - Match in middle: ~0.4µs (prefilter finds it quickly)
  - No match: ~35µs (must scan entire text)

The bottleneck is Bitap running for each prefilter candidate. Common letters like 'l', 'o' appear frequently, generating many candidates.

### Recent Optimizations (2026)

#### 1. Unicode Digit Prefilter Fix
- Pattern: `\d+`
- Issue: Unicode-aware digit matching was slow
- Fix: Use ASCII-only bytes for digit detection
- Result: 27µs → 2µs (15x faster)

#### 2. Greedy Dot-Star Instant Match
- Pattern: `.*`, `^.*$`, `.*$`
- Issue: NFA simulation was slow for patterns that always match
- Fix: Added `is_pure_greedy_dotstar()` detection in NFA, returns instant match
- Result: 12µs → 42ns (300x faster)

#### 3. DFA with Capturing Groups
- Pattern: `(?m)^(.*)test$`
- Issue: Capturing groups prevented DFA usage
- Fix: Allow DFA for patterns with capturing groups when possible
- Result: 1.5ms → 83ns (18000x faster)

#### 4. Greedy Prefix Optimization (.*SUFFIX)
- Pattern: `.*test`, `.*test~2`
- Issue: O(n²) behavior - greedy `.*` tries many ending positions with fuzzy matching at each
- Fix: Find suffix first using reverse search (`rfind` for exact, `find_rev` for fuzzy), then `.*` automatically matches everything before it
- Result: O(n) instead of O(n²)

Key insight: For `.*SUFFIX` patterns, finding SUFFIX from the right (using reverse search) and letting `.*` match everything before it avoids the combinatorial explosion of trying many ending positions.

## Future Optimization Opportunities

1. More selective prefilters (currently limited by pattern length)
2. Parallel processing for very long texts
3. SIMD improvements to Bitap (AVX2/NEON already implemented)
4. **`find_iter` for `LITERAL.*LITERAL`**: `find`/`is_match` reach parity with
   the regex crate via the `PrefixDotStarSuffix` two-literal-search fast path,
   but `find_iter` still materializes every match through the DFA's
   all-matches scan. A dot-star iterator that repeatedly locates the next
   literal pair could make `find_iter` O(matches) instead of O(text).
5. **Bounded dot-repeats** (`quick.{1,3}fox`): the shape detector rejects
   bounded middle repeats (falls back to the DFA); an interval-aware search
   would extend the literal-pair trick to them.

## Pathological Pattern Benchmark

Pathological patterns like `.*a|b` on text of all 'b's can cause O(n²) behavior in naive implementations because each match requires re-scanning from its start position. This benchmark compares three all-matches algorithms.

Runs as part of `bench_suite` (`cargo bench --bench bench_suite`), single-shot
timings.

### Results (Pattern `.*a|b` on text of 'b's)

| Text Size | find_all | two_pass | hardened | Speedup |
|-----------|----------|----------|----------|---------|
| 1,000 bytes | 2.1 ms | 2.1 ms | **0.1 ms** | 21x |
| 5,000 bytes | 53.7 ms | 53.4 ms | **0.3 ms** | 179x |
| 10,000 bytes | 213.6 ms | 212.9 ms | **0.6 ms** | **356x** |

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

### When to Use Each Algorithm

`Dfa` is constructed from a parsed/lowered pattern (as in
`benches/bench_suite.rs`):

```rust
use fuzzy_regex::compiler::build_nfa;
use fuzzy_regex::engine::{Dfa, FuzzyBridge};
use fuzzy_regex::ir::lower;
use fuzzy_regex::parser::parse;

fn make_dfa(pattern: &str) -> Option<Dfa> {
    let ast = parse(pattern).unwrap();
    let hir = lower(&ast, 0);
    let (nfa, literals) = build_nfa(&hir);
    let bridge = if literals.is_empty() {
        None
    } else {
        FuzzyBridge::new(&literals, None, None, false, false)
    };
    Dfa::from_nfa(&nfa, bridge.as_ref(), false, false, 1.0)
}

let mut dfa = make_dfa(".*a|b").unwrap();

// Default: smart selection based on pattern
let matches = dfa.find_all(text);

// Explicit two-pass: good when prefilter is effective
let matches = dfa.find_all_two_pass(text);

// Explicit hardened: critical for pathological patterns
let matches = dfa.find_all_hardened(text);
```

## Running Benchmarks

All benchmark harnesses live in `benches/` and are registered as `[[bench]]`
targets with `harness = false` (run the binary directly).

```bash
# Comprehensive suite: vs regex crate, streaming, pathological
cargo bench --bench bench_suite

# Quick performance overview (compile / short / long / edit scaling / DNA)
cargo bench --bench quick_bench

# Micro-benchmark used for the optimization findings
cargo bench --bench mini_bench

# Comparison vs mrab-regex (Rust side)
cargo bench --bench bench_vs_mrab

# Full vs mrab side-by-side table (Rust + Python)
bash benches/mrab_compare.sh

# Unicode benchmarks
cargo bench --bench bench_unicode

# Legacy comparison harnesses
cargo bench --bench bench_vs_regex
cargo bench --bench compare_bench
cargo bench --bench compare_std

# Criterion micro-benchmark
cargo bench --bench fuzzy_benchmarks
```

## Testing Performance

To verify optimizations:

```bash
# Run all tests
cargo test --all-features

# Run linter
cargo clippy --all-features -- -D warnings

# Format code
cargo fmt --all
```

## Notes

- All benchmarks run in release mode with LTO and a single codegen unit
- SIMD is enabled by default (`default = ["simd"]`)
- The `vs regex` tables measure `is_match` (first match); older revisions of
  this file reported `find_iter`-based numbers, so direct comparisons with
  earlier tables are not meaningful
- `bench_suite` uses automatic calibration so iteration counts vary by case

## References

- [docs/RE_SHARP_OPTIMIZATIONS.md](docs/RE_SHARP_OPTIMIZATIONS.md) - detailed documentation of all optimizations
- [docs/OPT_FUZZ_FINDINGS.md](docs/OPT_FUZZ_FINDINGS.md) - fuzzy matching correctness findings
