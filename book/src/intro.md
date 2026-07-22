# Introduction

`fuzzy-regex` is a high-performance fuzzy regular expression engine written in Rust. It combines traditional regex constructs with approximate string matching using Damerau-Levenshtein automata and the Bitap algorithm.

## Key Features

- **Fuzzy Matching**: Match strings with configurable edit distance tolerance (insertions, deletions, substitutions, transpositions)
- **Full Regex Support**: Character classes, quantifiers, groups, alternation, anchors, lookahead/lookbehind
- **Per-Segment Fuzziness**: Control fuzziness for individual parts of a pattern
- **Shared Edit Budgets**: Constrain total edits across multi-piece expressions using group-level fuzziness
- **Capture Groups**: Named and numbered capture groups with fuzzy matching
- **Similarity Scoring**: Get match quality scores (0.0 - 1.0)
- **Streaming API**: Process large files and network streams incrementally
- **High Performance**: Bitap algorithm for patterns ≤64 chars, SIMD optimizations
- **Unicode Support**: Full Unicode support including case-insensitive matching

## Why Fuzzy Regex?

Traditional regex requires exact matches. Fuzzy regex allows for "close enough" matches:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Match "hello" with up to 1 edit
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    println!("{}", re.is_match("hello")); // Exact match
    println!("{}", re.is_match("hallo")); // 1 substitution
    println!("{}", re.is_match("helo")); // 1 deletion  
    println!("{}", re.is_match("helllo")); // 1 insertion
    println!("{}", re.is_match("hlelo")); // 1 transposition
}
```

## Performance

`fuzzy-regex` is designed for high performance:

- **Bitap Algorithm**: O(n×k) time complexity for patterns ≤64 characters
- **SIMD Optimizations**: ~2-10x speedup on supported platforms (AVX2, NEON)
- **find_iter Fast Paths**: Specialized Aho-Corasick and literal search for iterating matches
- **Hardened Mode**: Guaranteed O(n) worst-case for pathological patterns
- **Streaming**: Process gigabytes of data without loading into memory

Typical throughput: **1.4-2.0 Gbps** for streaming fuzzy matching on modern hardware.

### Pathological Pattern Protection

Some patterns make the naive "find, advance, repeat" all-matches loop O(n²) —
for example `.*a|b` on a long run of `b`s, where each match's longest extent
needs an independent look-ahead. `find_all_hardened` uses a single-pass
all-matches scan that stays O(n) (linear) on such patterns, with results
identical to `find_iter`:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(".*a|b").unwrap();
    // Linear even on pathological input:
    let matches = re.find_all_hardened("bbbbbbbb");
    assert_eq!(matches.len(), 8);
}
```

(A single search — `find` / `is_match` — is always O(n): the engine is a
nonbacktracking automaton, so it has no catastrophic-backtracking blowup.)

## Ecosystem

- **MSRV**: Rust 1.88+
- **Features**: SIMD (default), mimalloc (optional)
- **Dependencies**: Zero runtime dependencies
