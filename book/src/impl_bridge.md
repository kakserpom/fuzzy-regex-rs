# Fuzzy Bridge

Connecting regex engine with fuzzy matching components.

## Purpose

The Fuzzy Bridge coordinates between the NFA-based regex engine and specialized fuzzy matchers (Bitap, Levenshtein NFA).

## Responsibilities

1. **Literal Extraction**: Extract exact-match literals from patterns for pre-filtering
2. **Search Coordination**: Order and combine multiple matchers
3. **Result Merging**: Combine and rank results from different matchers

## Architecture

```
Regex Pattern:  hello (?:world){e<=1}
                    │
                    ▼
           ┌────────────────┐
           │ Fuzzy Bridge   │
           └───────┬────────┘
                   │
       ┌───────────┴───────────┐
       ▼                       ▼
┌──────────────┐      ┌──────────────┐
│    DFA       │      │   Fuzzy      │
│  (exact)     │      │   Matcher    │
│  "hello"     │      │ (Bitap/NFA)  │
└──────────────┘      │ "world"{e<=1}│
                      └──────────────┘
```

## Literal Extraction

```rust
// Pattern: "prefix (?:target){e<=1} suffix"
// Extracted literals: ["prefix", "suffix"]
```

### Extraction Strategy

1. Identify fuzzy segments in pattern
2. Extract surrounding exact literals
3. Use literals for:
   - Pre-filtering text positions
   - Quick rejection of non-matching areas

## Search Flow

### Simple Fuzzy Pattern

For `(?:hello){e<=1}`:

1. Use Bitap directly (pattern ≤64 chars)
2. Or build Levenshtein NFA (longer patterns)

### Mixed Pattern

For `exact (?:fuzzy){e<=1}`:

1. Use DFA to find "exact" quickly
2. For each "exact" match, check surrounding for fuzzy match

### Complex Pattern

For `(?:a){e<=1}(?:b){e<=1}`:

1. Find all fuzzy matches for "a"
2. Find all fuzzy matches for "b"
3. Check if they can be combined

## Optimization Techniques

### Prefiltering

Use extracted literals to quickly skip non-matching regions:

```rust
// Text: "blah blah prefix target suffix blah"
// Literal "prefix" found at position X
// Only check fuzzy "target" around position X
```

### Early Termination

Stop searching when match found (for find/first):

```rust
// First match found, don't continue
```

### Caching

Cache compiled matchers for reuse:
```rust
// Bitap matcher cached for pattern
```
