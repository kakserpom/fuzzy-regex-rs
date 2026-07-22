# Damerau-Levenshtein NFA

Fuzzy matching via finite automata for longer patterns.

## Overview

For patterns longer than 64 characters or when complex regex features are needed, fuzzy-regex uses a Damerau-Levenshtein NFA. This is an extension of the classic Damerau-Levenshtein automaton.

## Damerau-Levenshtein Automaton

The Damerau-Levenshtein automaton for a pattern P and max distance k accepts all strings within k edits of P.

### State Representation

```rust
// Each state represents a position in the pattern
// plus the number of errors used so far
struct NfaState {
    pattern_position: usize,  // Position in pattern
    errors_used: usize,       // Errors consumed
}
```

### Transitions

For each input character, the NFA can transition to:

1. **Match**: Next character in pattern (0 errors)
2. **Insertion**: Stay at current position (1 error)
3. **Deletion**: Skip pattern character (1 error)
4. **Substitution**: Move to next with 1 error
5. **Transposition**: Swap next two characters (special case)

## Building the NFA

```rust
// Pattern: "abc" with k=1
// States: (0,0) → (1,0) → (2,0) → (3,0)  [exact path]
//         (0,0) → (1,1) → ...             [with errors]
```

### Construction Algorithm

1. Create initial state at position 0
2. For each pattern character, create states for:
   - Exact match (no error)
   - Insertion (adds error)
   - Deletion (adds error)
   - Substitution (adds error)
3. Connect states with transitions
4. Mark accepting states

## Performance Characteristics

| Aspect | Damerau-Levenshtein NFA |
|--------|----------------|
| Time Complexity | O(n × k × m) |
| Space Complexity | O(k × m) |
| Pattern Length | Unlimited |
| Features | Full regex |

Where:
- n = text length
- k = max edits
- m = pattern length

## Comparison with Bitap

| Feature | Bitap | Damerau-Levenshtein NFA |
|---------|-------|-----------------|
| Max Pattern Length | 64 | Unlimited |
| Time Complexity | O(n×k) | O(n×k×m) |
| SIMD Support | Yes | Limited |
| Regex Features | Limited | Full |
| Memory Usage | Low | Higher |

## When Used

- Pattern length > 64 characters
- Complex regex features (lookahead, backreferences)
- Alternation with fuzzy segments
