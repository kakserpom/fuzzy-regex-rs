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
