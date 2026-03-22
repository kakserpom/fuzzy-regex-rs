# DFA Optimization

Fast path for exact and non-fuzzy patterns.

## Deterministic Finite Automaton

The DFA provides the fastest matching path for patterns that don't require fuzzy matching.

## When DFA is Used

- Exact match patterns (no fuzziness)
- Simple character classes
- Quantifiers without fuzzy segments

## DFA Properties

| Property | Value |
|----------|-------|
| Time Complexity | O(n) |
| Space Complexity | O(states) |
| Backtracking | None |
| Memory | Deterministic |

## Implementation

### State Machine

```rust
// DFA state contains:
// - Transitions for each possible input
// - Whether state is accepting
// - Associated match information
```

### Simulation

```rust
// For each input character:
// 1. Look up next state from current state
// 2. If no transition, stop (no match)
// 3. If accepting state, record match position
// 4. Continue until end of input
```

## Fast Path Integration

```
Pattern: hello{e<=1}
              │
              ▼
    ┌─────────────────┐
    │ Has Fuzziness?  │
    └────────┬────────┘
             │
      ┌──────┴──────┐
      ▼             ▼
   Yes            No
     │             │
     ▼             ▼
   NFA/          DFA
   Bitap
```

## Limitations

The DFA cannot handle:
- Fuzzy matching
- Lookahead/lookbehind
- Backreferences
- Capture groups (for position info)

For these, the NFA engine is used.

## Optimization Techniques

### State Minimization

Reduce number of states:

```rust
// Before: 100 states
// After minimization: 50 states
```

### Lazy Evaluation

Build DFA states on-demand:

```rust
// Only build states that are actually traversed
```

### Caching

Cache DFA for reuse:

```rust
// Reuse compiled DFA across searches
```

## All-Matches Algorithms

The DFA supports multiple algorithms for finding all matches.

### Three-Pass (Default)

The standard approach: find a match, advance, repeat.

```rust
let matches = dfa.find_all(text);
// Time: O(n²) worst case for pathological patterns
```

### Two-Pass Algorithm

Uses a reverse prefilter to find candidates efficiently:

1. **Pass 1**: Scan right-to-left, finding candidate positions
2. **Pass 2**: Verify matches at each candidate

```rust
let matches = dfa.find_all_two_pass(text);
// Prefilter reduces candidates in Pass 1
// Still O(n²) worst case in Pass 2
```

### Hardened Mode (O(n))

Tracks all active DFA states simultaneously:

1. Maintain set of (state, start_position) pairs
2. Process each character once, updating all states
3. When no continuation possible, emit match
4. Continue from next position

```rust
let matches = dfa.find_all_hardened(text);
// Guaranteed O(n) - each character processed once
```

### Prefilter Types

| Type | Description |
|------|-------------|
| SingleByte | `memrchr` for one byte |
| TwoBytes | `memrchr` for two bytes |
| ThreeBytes | `memrchr` for three bytes |
| Teddy | SIMD-accelerated multi-byte |

### When to Use

| Scenario | Recommended |
|----------|--------------|
| Few matches, well-behaved pattern | Default |
| Prefilter effective | Two-pass |
| Pathological patterns | Hardened |
