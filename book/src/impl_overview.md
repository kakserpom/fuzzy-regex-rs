# Architecture Overview

How fuzzy-regex works internally.

## High-Level Architecture

```
┌─────────────────────────────────────────────┐
│              FuzzyRegex                      │
│         (Public API Layer)                   │
└──────────────────┬──────────────────────────┘
                   │
        ┌──────────┴──────────┐
        ▼                     ▼
┌───────────────┐   ┌─────────────────┐
│     DFA       │   │   NFA Engine   │
│  (Fast Path)  │   │  (Full Engine) │
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
│  Bitap   │         │  Lev.    │
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

### 3. Matching Engines

#### DFA (Deterministic Finite Automaton)
- Fast path for exact/non-fuzzy patterns
- No backtracking
- Limited feature support

#### NFA (Non-deterministic Finite Automaton)
- Full regex feature support
- Backtracking engine
- Supports fuzzy matching

#### Bitap Algorithm
- Optimized for short patterns (≤64 chars)
- O(n×k) time complexity
- SIMD-accelerated

#### Levenshtein NFA
- Fuzzy matching via automata
- Supports all edit types
- Used for longer patterns

### 4. Fuzzy Bridge
- Connects NFA to fuzzy matchers
- Extracts literals for pre-filtering
- Coordinates multiple matchers
