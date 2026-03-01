# Summary

[Introduction](./intro.md)

# User Guide

- [Installation](./installation.md)
- [Quick Start](./quick_start.md)
- [Fuzzy Matching Basics](./fuzzy_basics.md)
  - [Edit Distance](./fuzzy_edit_distance.md)
  - [Fuzziness Markers](./fuzzy_markers.md)
  - [Cost-Based Matching](./fuzzy_cost.md)
- [Pattern Syntax](./pattern_syntax.md)
  - [Character Classes](./pattern_char_classes.md)
  - [Quantifiers](./pattern_quantifiers.md)
  - [Groups and Alternation](./pattern_groups.md)
  - [Anchors and Boundaries](./pattern_anchors.md)
  - [Lookahead and Lookbehind](./pattern_lookaround.md)
  - [Atomic Groups and Possessive Quantifiers](./pattern_atomic.md)
- [API Reference](./api_reference.md)
  - [FuzzyRegex](./api_fuzzy_regex.md)
  - [FuzzyRegexBuilder](./api_builder.md)
  - [Match Results](./api_match.md)
  - [Streaming API](./api_streaming.md)
- [Advanced Features](./advanced.md)
  - [Capture Groups](./advanced_captures.md)
  - [Partial Matching](./advanced_partial.md)
  - [Word Lists](./advanced_wordlists.md)
  - [Compatibility Layer](./advanced_compat.md)

# Implementation Guide

- [Architecture Overview](./impl_overview.md)
- [Bitap Algorithm](./impl_bitap.md)
- [Levenshtein NFA](./impl_nfa.md)
- [Fuzzy Bridge](./impl_bridge.md)
- [DFA Optimization](./impl_dfa.md)
- [Streaming Implementation](./impl_streaming.md)

# Performance

- [Algorithm Selection](./perf_algorithms.md)
- [SIMD Optimizations](./perf_simd.md)
- [Performance Tips](./perf_tips.md)

# Appendix

- [Pattern Syntax Reference](./appendix_syntax.md)
- [Error Messages](./appendix_errors.md)
- [Migration Guide](./appendix_migration.md)
