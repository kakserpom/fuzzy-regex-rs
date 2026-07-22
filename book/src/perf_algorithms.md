# Algorithm Selection

How fuzzy-regex chooses the matching algorithm.

## Decision Tree

```text
Pattern Analysis
      │
      ▼
┌──────────────────┐
│ Is exact match?  │
└────────┬─────────┘
          │
      ┌────┴────┐
      ▼         ▼
     Yes       No
       │         │
       ▼         ▼
    DFA    ┌────────────────┐
           │ Pure greedy .*? │
           └────────┬────────┘
                    │
               ┌────┴────┐
               ▼         ▼
              Yes       No
               │         │
               ▼         ▼
         Instant      ┌────────────────┐
          Match      │ Pattern ≤64?   │
                     └────────┬────────┘
                              │
                         ┌────┴────┐
                         ▼         ▼
                        Yes       No
                         │         │
                         ▼         ▼
                       Bitap    Damerau-Levenshtein
                                  NFA
```

## Algorithm Comparison

| Algorithm | Use Case | Complexity | Features |
|-----------|----------|------------|----------|
| DFA | Exact patterns, capturing groups | O(n) | Limited |
| Instant Match | Pure greedy `.*` | O(1) | Limited |
| Bitap | Short fuzzy (≤64 chars) | O(n×k) | Most |
| Damerau-Levenshtein NFA | Long fuzzy patterns | O(n×k×m) | Full |

## Automatic Selection

The library automatically selects based on:

1. **Pattern length**: Bitap for ≤64 chars
2. **Fuzzy complexity**: Cost-based vs simple
3. **Regex features**: DFA can't do lookahead
4. **Streaming**: Different code path
5. **Greedy patterns**: Instant match for `.*`, `^.*$`, `.*$`
6. **Greedy suffix patterns**: `.*SUFFIX` uses reverse search

## Special Optimizations

### Pure Greedy Dot-Star

Patterns like `.*`, `^.*$`, `.*$` always match. The engine detects these and returns instantly without scanning:

```rust,ignore
let re = FuzzyRegex::new(".*").unwrap();
// Returns match immediately - no text scanning
```

### Greedy Prefix with Suffix

Patterns like `.*test` or `.*test~2` use reverse search to avoid O(n²) behavior:

```rust,ignore
let re = FuzzyRegex::new(".*test").unwrap();
// Finds "test" from the right, then .* matches everything before it
// O(n) instead of O(n²)
```

### DFA with Capturing Groups

DFA now works with capturing groups like `(?m)^(.*)test$`:

```rust,ignore
let re = FuzzyRegex::new("(?m)^(.*)test$").unwrap();
// Uses DFA - much faster than NFA
```text

## All-Matches Algorithms

When finding all matches in text (`find_iter`, `find_all`), fuzzy-regex supports multiple algorithms optimized for different patterns.

### The Problem

Some patterns produce many overlapping matches, causing naive "find, advance, repeat" to be O(n²):

```text
// Pattern: .*a|b on text: "bbbbbbbb"
// Each 'b' matches individually → O(n) matches × O(n) scan = O(n²)
```

### Available Algorithms

| Algorithm | Complexity | Best For |
|-----------|------------|----------|
| `find_all` | O(n²) worst case | Simple patterns, few matches |
| `find_all_two_pass` | O(n²) worst case | When prefilter is effective |
| `find_all_hardened` | **O(n)** guaranteed | Pathological patterns |

### Two-Pass Algorithm

Uses a reverse prefilter to find candidate positions, then verifies matches:

1. **Pass 1**: Scan right-to-left using prefilter (memchr, Teddy, etc.)
2. **Pass 2**: For each candidate, find the match

```rust,ignore
let mut dfa = Dfa::new(".*a|b").unwrap();
let matches = dfa.find_all_two_pass("bbbbbb");
```

### Hardened Mode (O(n))

Tracks all active DFA states simultaneously as we scan left-to-right:

```rust,ignore
let mut dfa = Dfa::new(".*a|b").unwrap();
let matches = dfa.find_all_hardened("bbbbbb");
```

**How it works:**

1. Start with one state at current position
2. Process all characters, tracking active states
3. When no state can continue, emit the leftmost match
4. Move to next position, repeat

### Benchmark: Pathological Pattern

Pattern `.*a|b` on text of all 'b's (worst case):

| Text Size | find_all | two_pass | hardened | Speedup |
|-----------|----------|----------|----------|---------|
| 1,000 bytes | 1.08s | 1.10s | **69ms** | 15x |
| 5,000 bytes | 5.47s | 5.43s | **69ms** | 79x |
| 10,000 bytes | 10.76s | 10.85s | **69ms** | **156x** |

### When to Use Each

```rust,ignore
// Default: smart selection based on pattern
let matches = dfa.find_all(text);

// Explicit two-pass: good when prefilter is effective
let matches = dfa.find_all_two_pass(text);

// Explicit hardened: critical for pathological patterns
let matches = dfa.find_all_hardened(text);
```

### Prefilter Types

The two-pass algorithm uses prefilters optimized for different patterns:

| Prefilter | Use Case | Method |
|-----------|----------|--------|
| SingleByte | Single literal | `memrchr` |
| TwoBytes | Two literals | `memrchr` for both |
| ThreeBytes | Three literals | `memrchr` for all |
| Teddy | Many alternatives | SIMD range matching |

## Manual Override

Not currently exposed, but internal selection can be inspected:

```rust,ignore
// Check which engine was used
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
if re.supports_streaming() {
    // Using Bitap
}
```
