# Performance Optimization Findings

## Summary

This document records the performance investigations and optimizations attempted for fuzzy-regex.

## Benchmark Configuration

All benchmarks run with `cargo run --example mini_bench --release`:
- Short text: "The quick brown fox jumps over the lazy dog." (~43 chars)
- Long text: 10KB of text
- Pattern: `(?:quikc){e<=1}` or `(?:qwick){e<=2}`

## Baseline Performance

```
short text, 1 edit:         1.5 µs
short text, 2 edits:        2.5-3.0 µs
long text, 2 edits:        42-45 µs
long text, no match:       33-35 µs
```

## Investigations & Results

### 1. SmallVec vs Vec

**Finding**: Vec is 2.5-3.6x faster than SmallVec for all common cases.

**Conclusion**: Use standard `Vec` instead of `SmallVec`.

### 2. SmartString vs String

**Finding**: String is 3-6x faster than SmartString (except for very short strings <23 bytes).

**Conclusion**: Use standard `String` instead of `SmartString`.

### 3. Mimalloc vs System Allocator

**Finding**: System allocator is faster for this workload - mimalloc added overhead.

**Conclusion**: Keep system allocator.

### 4. Option<Vec> for handler_overrides

**Attempted**: Using `Option<Vec>` to avoid cloning empty Vecs.

**Finding**: Added branch overhead and made it slower.

**Conclusion**: Keep `Vec` with empty check before clone.

### 5. EditCounts Copy Derive

**Attempted**: Made `EditCounts` `Copy`-able to reduce clone() overhead.

**Finding**: Caused ~2x regression on long text benchmarks. The clone() is cheap enough that explicit copying hurts performance (likely due to register pressure or cache effects).

**Conclusion**: Keep `EditCounts` as `Clone` only - reverted.

### 6. Struct Field Ordering

**Applied**: Optimized struct field ordering for better cache locality.

**Finding**: Minor improvements.

## Final State

Successfully removed dependencies:
- `smallvec` → replaced with `Vec`
- `smartstring` → replaced with `String`
- `smartcow` → replaced with `std::borrow::Cow`

All tests pass. Performance maintained at baseline.

## What Didn't Help

1. Making EditCounts Copy - hurt performance
2. Using Option<Vec> for empty checks - hurt performance  
3. Mimalloc allocator - hurt performance
4. SmartString for short strings - hurt performance

## What Helped

1. Removing smallvec/smartstring/smartcow - reduced dependencies, no performance loss
2. Struct field ordering - minor improvement
3. Avoiding unnecessary clones where practical
4. **GuardNFA ASCII optimization** - Added byte-level path for ASCII text, avoiding Vec<char> allocation. ~27% improvement for exact matches on ASCII text (45µs -> 33µs).
5. **Alternation fast path** - Added fast path in FuzzyRegex::find() for exact alternations (e.g., "cat|dog|bat"). This avoids the expensive find_iter -> find_all path. ~25x improvement (16µs -> 0.6µs).

## Investigation: Long Text Fuzzy Matching

After detailed profiling, found that long text fuzzy matching is fundamentally expensive:

- For pattern "lorem" with e<=1 in 4KB text:
  - Fuzzy prefilter finds ~500 candidate positions
  - Bitap runs ~70ns per candidate
  - Total: 500 * 70ns = 35µs (matches observed performance)

- When match exists at position 0: ~1.5µs (fast termination)
- When match exists in middle: ~0.4µs (prefilter finds it quickly)
- When no match: ~35µs (must scan entire text)

The bottleneck is the Bitap algorithm running for each prefilter candidate. The prefilter is already quite selective (4 bytes for e<=1), but common letters like 'l', 'o' appear frequently in text.

Further optimization would require:
1. More selective prefilters (pigeonhole requires pattern >= 6 chars)
2. Parallel processing for very long texts
3. SIMD improvements to Bitap (already has AVX2/NEON)
