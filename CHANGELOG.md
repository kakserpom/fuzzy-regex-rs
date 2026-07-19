# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-07-19

### Changed

- **Breaking (output):** `Match::fuzzy_counts()` / `fuzzy_changes()` (and the
  `Captures` equivalents) now return counts in `(substitutions, insertions,
  deletions)` order to match mrab-regex's `fuzzy_counts`. Code that relied on
  the previous ordering must be updated.

### Added

- Per-group edit budget for `{i<=N,d<=N,s<=N}` on non-capturing groups, with
  budgets shared across the group.
- End-anchored fuzzy matching now runs in near-constant time regardless of
  input size (see Performance).

### Fixed

- Feature parity with mrab-regex: word-boundary fuzzy metadata now carries the
  real edit counts and similarity; `{i<=k}` / `{d<=k}` / `{s<=k}` individual
  limits return the optimal (leftmost, fewest-edit, shortest-span) match.
- Correct several mrab divergences in alternation dedup, per-operation limits,
  and the `{cost}` / `{e}` rule interaction.
- Implement insertion in the `FuzzyChar` NFA step; fix quantifier-in-group
  budgets and standalone fuzzy-class substitution.
- Fix an invalid per-operation edit breakdown returned for over-budget windows,
  including non-ASCII (UTF-8) inputs.
- Fix UTF-8 panics on multi-byte text.

### Performance

- **End-anchor windowing:** fuzzy patterns ending in `$` (single-line) now
  search only a bounded window near the end of the text and shift the results
  back into absolute coordinates, so `find` / `find_iter` cost is independent
  of input length instead of a full O(n) scan (measured ~3500x faster on a
  ~20 KB input, and the gap widens with size). `find` and `find_iter` also
  now share this path and return consistent results.
- Cut redundant edit-distance DP in the multi-pattern Bitap matcher and gate
  the per-operation breakdown with a lighter distance-only pass, extended to
  non-ASCII windows.
- Additional `find_iter` fast paths for literal, alternation (Aho-Corasick),
  and character-class + literal patterns.

## [0.1.0]

- Initial release.

[0.2.0]: https://github.com/kakserpom/fuzzy-regex-rs/releases/tag/v0.2.0
[0.1.0]: https://github.com/kakserpom/fuzzy-regex-rs/releases/tag/v0.1.0
