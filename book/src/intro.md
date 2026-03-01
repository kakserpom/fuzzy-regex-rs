# Introduction

`fuzzy-regex` is a high-performance fuzzy regular expression engine written in Rust. It combines traditional regex constructs with approximate string matching using Damerau-Levenshtein automata and the Bitap algorithm.

## Key Features

- **Fuzzy Matching**: Match strings with configurable edit distance tolerance (insertions, deletions, substitutions, transpositions)
- **Full Regex Support**: Character classes, quantifiers, groups, alternation, anchors, lookahead/lookbehind
- **Per-Segment Fuzziness**: Control fuzziness for individual parts of a pattern
- **Capture Groups**: Named and numbered capture groups with fuzzy matching
- **Similarity Scoring**: Get match quality scores (0.0 - 1.0)
- **Streaming API**: Process large files and network streams incrementally
- **High Performance**: Bitap algorithm for patterns ≤64 chars, SIMD optimizations
- **Unicode Support**: Full Unicode support including case-insensitive matching

## Why Fuzzy Regex?

Traditional regex requires exact matches. Fuzzy regex allows for "close enough" matches:

```rust
use fuzzy_regex::FuzzyRegex;

// Match "hello" with up to 1 edit
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

re.is_match("hello"); // Exact match
re.is_match("hallo"); // 1 substitution
re.is_match("helo"); // 1 deletion  
re.is_match("helllo"); // 1 insertion
re.is_match("hlelo"); // 1 transposition
```

## Performance

`fuzzy-regex` is designed for high performance:

- **Bitap Algorithm**: O(n×k) time complexity for patterns ≤64 characters
- **SIMD Optimizations**: ~2x speedup on supported platforms
- **Streaming**: Process gigabytes of data without loading into memory

Typical throughput: **1.4-2.0 Gbps** for streaming fuzzy matching on modern hardware.

## Ecosystem

- **MSRV**: Rust 1.88+
- **Features**: SIMD (default), mimalloc (optional)
- **Dependencies**: Zero runtime dependencies
