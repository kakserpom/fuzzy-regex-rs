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
         │ Pattern ≤64?   │
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
| DFA | Exact patterns | O(n) | Limited |
| Bitap | Short fuzzy (≤64 chars) | O(n×k) | Most |
| Damerau-Levenshtein NFA | Long fuzzy patterns | O(n×k×m) | Full |

## Automatic Selection

The library automatically selects based on:

1. **Pattern length**: Bitap for ≤64 chars
2. **Fuzzy complexity**: Cost-based vs simple
3. **Regex features**: DFA can't do lookahead
4. **Streaming**: Different code path

## Manual Override

Not currently exposed, but internal selection can be inspected:

```rust
// Check which engine was used
let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
if re.supports_streaming() {
    // Using Bitap
}
```
