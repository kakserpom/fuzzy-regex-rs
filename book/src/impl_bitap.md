# Bitap Algorithm

Fast fuzzy matching algorithm for short patterns.

## Overview

The Bitap algorithm (also known as the Shift-Or algorithm) is used for fuzzy matching of short patterns (≤64 characters). It achieves O(n×k) time complexity where n is text length and k is maximum edits.

## How It Works

Bitap uses bitmasks to track which positions can match:

1. **Initialize**: Create a bitmask for each character in the pattern
2. **Scan**: For each text character, update the bitmask
3. **Match**: When bitmask indicates a match, extract the match

## Bitap State

```rust
// Simplified bitap state representation
struct BitapState {
    // Bits representing current match positions
    // For pattern "hello":
    // Position:    5   4   3   2   1   0
    // Bits:        0   0   0   0   0   1  = "h"
    //              0   0   0   0   1   0  = "e"
    //              ... continue ...
}
```

## Key Properties

### Pattern Length Limit
- Maximum 64 characters (uses 64-bit integers)
- Longer patterns use Damerau-Levenshtein NFA

### Edit Distance
- Supports insertions, deletions, substitutions
- Transpositions handled specially

### SIMD Acceleration

Modern implementations use SIMD for parallelism:

```rust
// SIMD processes multiple characters at once
// 256-bit AVX2: process 32 bytes per iteration
// 512-bit AVX512: process 64 bytes per iteration
```

## Performance

| Pattern Length | Text Size | Throughput |
|----------------|-----------|------------|
| 14 chars | 200KB | ~1.5 Gbps |
| 14 chars | 1MB | ~1.4 Gbps |
| 14 chars, k=1 | 200KB | ~2.0 Gbps |
| 14 chars, k=3 | 200KB | ~700 Mbps |

## When Bitap is Used

- Pattern length ≤ 64 characters
- Simple fuzzy matching (no complex regex features)
- Exact matching benefits from optimizations

## Implementation Details

### State Machine

```rust
// Each iteration:
// 1. Shift state right by 1
// 2. OR with pattern mask for current char
// 3. Check for match (zero in certain bits)
```

### Handling Errors

Bitap extends to handle errors by maintaining multiple state masks (one per allowed error):

```rust
// For k errors, maintain k+1 states
// state[0] = exact match
// state[1] = 1 error
// state[k] = k errors
```
