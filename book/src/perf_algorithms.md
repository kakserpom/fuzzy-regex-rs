# Algorithm Selection

How fuzzy-regex chooses the matching algorithm.

## Decision Tree

```
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

```rust
let re = FuzzyRegex::new(".*").unwrap();
// Returns match immediately - no text scanning
```

### Greedy Prefix with Suffix

Patterns like `.*test` or `.*test~2` use reverse search to avoid O(n²) behavior:

```rust
let re = FuzzyRegex::new(".*test").unwrap();
// Finds "test" from the right, then .* matches everything before it
// O(n) instead of O(n²)
```

### DFA with Capturing Groups

DFA now works with capturing groups like `(?m)^(.*)test$`:

```rust
let re = FuzzyRegex::new("(?m)^(.*)test$").unwrap();
// Uses DFA - much faster than NFA
```

## Manual Override

Not currently exposed, but internal selection can be inspected:

```rust
// Check which engine was used
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
if re.supports_streaming() {
    // Using Bitap
}
```
