# Performance Optimization Findings

This document records the performance investigations and optimizations attempted for fuzzy-regex.

## Benchmark Configuration

All benchmarks run with `cargo run --example mini_bench --release`:
- Short text: "The quick brown fox jumps over the lazy dog." (~43 chars)
- Long text: 10KB of text
- Pattern: `(?:quikc){e<=1}` or `(?:qwick){e<=2}`

## Current Performance

```
short text, 1 edit:         1.5 µs
short text, 2 edits:        2.6 µs
long text, 2 edits:        43-45 µs
long text, no match:       33-35 µs
```

## Optimizations Applied

### 1. Dependencies Removed
- `smallvec` → replaced with `Vec`
- `smartstring` → replaced with `String`
- `smartcow` → replaced with `std::borrow::Cow`

### 2. GuardNFA ASCII Optimization
Added byte-level path for ASCII text, avoiding `Vec<char>` allocation.
- ~27% improvement for exact matches on ASCII text

### 3. Alternation Fast Path
Added direct path in `FuzzyRegex::find()` for exact alternations (e.g., "cat|dog|bat").
- Avoids expensive `find_iter -> find_all` path
- ~25x improvement (16µs -> 0.6µs)

### 4. Pigeonhole Prefilter Extension
Extended pigeonhole prefilter to work with shorter patterns.
- Changed threshold from 10+ chars to `2*(k+1)` chars
- For k=1: now uses pigeonhole for patterns >= 4 chars
- For k=2: now uses pigeonhole for patterns >= 6 chars
- ~38% improvement for long text fuzzy matching

### 5. SIMD Batch Parallel
Use batch parallel SIMD search when prefilter selectivity <= 2.
- Uses lazy search otherwise (better for early termination)

## Investigations That Didn't Help

1. **EditCounts Copy** - Caused ~2x regression (reverted)
2. **Option<Vec>** - Added branch overhead (reverted)
3. **Mimalloc allocator** - Added overhead (reverted)
4. **SmartString** - Slower than String for most cases

## Long Text Fuzzy Matching Analysis

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

## Future Optimization Opportunities

1. More selective prefilters (currently limited by pattern length)
2. Parallel processing for very long texts
3. SIMD improvements to Bitap (AVX2/NEON already implemented)
