# Architecture Overview

How fuzzy-regex works internally.

## High-Level Architecture

```text
┌─────────────────────────────────────────────┐
│              FuzzyRegex                      │
│         (Public API Layer)                   │
│                                              │
│  ┌────────────────────────────────────────┐  │
│  │        Fast Path Optimizations         │  │
│  │  • Pure greedy .* → instant match     │  │
│  │  • .*SUFFIX → reverse search          │  │
│  │  • Word boundaries → direct search    │  │
│  └────────────────────────────────────────┘  │
└──────────────────┬──────────────────────────┘
                  │
         ┌────────┴──────────┐
         ▼                     ▼
┌───────────────┐   ┌─────────────────┐
│     DFA       │   │   NFA Engine   │
│  (Fast Path)  │   │  (Full Engine) │
│ + Capturing   │   │                │
│    Groups     │   │                │
└───────┬───────┘   └────────┬────────┘
        │                    │
        └────────┬───────────┘
                 ▼
        ┌─────────────────┐
        │  Fuzzy Bridge  │
        └────────┬────────┘
                 │
      ┌───────────┴───────────┐
      ▼                       ▼
┌──────────┐         ┌──────────┐
│  Bitap   │         │   D-L    │
│ Matcher  │         │   NFA    │
└──────────┘         └──────────┘
```

## Components

### 1. Parser
- Tokenizes regex patterns
- Builds Abstract Syntax Tree (AST)
- Handles fuzzy syntax extensions

### 2. Compiler
- Converts AST to Intermediate Representation (IR)
- Optimizes patterns
- Handles fuzzy matching compilation

### 3. Fast Path Optimizations

Before going to the main engines, FuzzyRegex checks for special patterns:

- **Pure greedy dot-star**: `.*`, `^.*$`, `.*$` return instantly
- **Greedy prefix with suffix**: `.*test` uses reverse search (O(n) vs O(n²))
- **Word boundaries**: Direct literal search with boundary checks
- **DFA compatibility**: Patterns with capturing groups now use DFA

### 4. Matching Engines

#### DFA (Deterministic Finite Automaton)
- Fast path for exact/non-fuzzy patterns
- No backtracking
- Supports capturing groups
- Limited feature support

#### NFA (Non-deterministic Finite Automaton)
- Full regex feature support
- Backtracking engine
- Supports fuzzy matching

#### Bitap Algorithm
- Optimized for short patterns (≤64 chars)
- O(n×k) time complexity
- SIMD-accelerated

#### Damerau-Levenshtein NFA
- Fuzzy matching via automata
- Supports all edit types
- Used for longer patterns

### 5. Fuzzy Bridge
- Connects NFA to fuzzy matchers
- Extracts literals for pre-filtering
- Coordinates multiple matchers
