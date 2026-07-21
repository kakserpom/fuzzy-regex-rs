# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Real expected values for the 73 mrab corpus "skeleton" cases (previously just a
  name + pattern with no assertion, so they verified nothing). These mirror
  mrab's compile-only tests (`repr(type(regex.compile(...))) == PATTERN_CLASS`),
  so the authentic expectation is compilation, now asserted via a new
  `compiles = true` corpus field (checked against the Python `regex` oracle). Of
  the 73: active compile-only tests now cover the cases both engines accept
  (including `\L<words>`, which fuzzy-regex compiles for deferred resolution via
  `set_word_list`), while patterns that mrab compiles but fuzzy-regex does not
  yet (reverse `(?r)`, `\N{...}`, recursion `(?0)`, `\m`/`\M`, combined
  constraints, fuzzy `{e}` on capturing groups) stay ignored with a documented
  GAP note. The 7 pre-existing active compile-only cases also gained explicit
  `compiles = true`; every corpus case now carries an assertion.
- A property-based consistency test (`tests/find_iter_consistency_proptest.rs`)
  that generates patterns from the atom/quantifier grammar which surfaced the
  fast-path divergences below and asserts `find(text) == find_iter(text).next()`
  in default (leftmost) matching. This locks in the invariant that `find`'s
  heuristic fast paths never disagree with the `find_iter` reference, catching
  any future regression. Complemented by a crate-internal `#[cfg(test)]` guard
  asserting the same equality across the crate's own unit tests.
- `MatchEndPolicy` and `FuzzyRegexBuilder::match_end_policy(...)` to choose how a
  fuzzy match's end is selected when several ends are valid within the edit
  budget. The default (`LongestWithinBudget`) preserves the historical
  longest-span behavior; `MinEdit` reports the tightest alignment (fewest edits,
  then closest-to-pattern-length, then shortest span), matching mrab-regex's
  minimal-error reporting for large/unlimited `{e}` budgets. For example,
  `(?:error){e}` against `"regex failure"` reports `[0, 8]` under the default and
  `[0, 5]` (`"regex"`) under `MinEdit`. `find`, `find_iter`, and `captures` agree
  under `MinEdit`.

### Fixed

- Unresolved `\L<name>` named-list references matched the empty string everywhere
  instead of matching nothing. `\L<name>` compiles to a placeholder resolved
  later via `set_word_list`; until a list was provided it lowered to an empty
  epsilon, so `\b\L<words>{e<=1}\b` on `"a dog x"` returned `(0,0)` (and
  `is_match` was `true`, `find_iter` yielded `(0,0),(1,1),…`). An unset list is an
  empty alternation and matches nothing, so all match entry points (`find`,
  `find_iter`, `is_match`, `captures`, `find_at`, `find_rev`, and the reverse/
  overlapping variants) now short-circuit to no match when any referenced
  `\L<name>` is still unresolved. Once the list is set via `set_word_list`,
  matching works as before.
- `captures`, `captures_at`, `captures_all_overlapping`, and `find_at` returned
  no match for `\L<name>` patterns even when the word list was resolved, because
  they use the NFA path (which does not expand named lists) rather than the
  word-list matcher that `find`/`find_iter` use. They are now word-list-aware and
  return the matched word (group 0), consistent with `find`.
- `find()` fast-path hijacking: the specialized linear-scan "shape" fast paths
  (currency, class-plus-with-literal, digit-sequence-with-separator) grabbed
  complex anchored or group-repeating patterns they cannot handle and returned
  truncated or missing matches that disagreed with `find_iter()`. Examples:
  `^\d(?:,\d)*$` on `"1,2,3"` returned `[0,3]` instead of `[0,5]`; leading-`$`
  patterns such as `^\$\d+$` returned no match. These heuristics are now skipped
  for patterns with a repeated multi-atom group, with anchors, or that lead with
  a literal, so the correct DFA/NFA path handles them; the fast paths still apply
  to their intended unanchored patterns (emails, dates, currency, IPs).
- DFA `find`/`find_all` discrepancy for end-anchored, empty-accepting patterns:
  `Dfa::find`'s prefilter path returned a bogus empty match at position 0
  (ignoring the end anchor) when the prefilter found no candidate, so e.g. `,?$`
  on `"b.,a"` returned `(0,0)` instead of the empty match at end `(4,4)`. It now
  probes the end position (and interior boundaries in multiline mode) via
  `find_at`, matching `find_all`/`find_iter`.
- Two related `find()` fast-path hijacks fixed alongside it: the end-anchored
  exact-literal `rfind` path fired for non-literal patterns (`[0-9]{2}(?:,\d)$`
  matched a lone trailing comma) and ignored the start anchor — it now requires a
  single fixed literal with no classes, branching, or `^`; and a bounded dot
  repeat (`^.{1,3}$`) was mis-detected as `.*` and matched the whole text — the
  "match everything" fast path now requires a genuinely unbounded dot repeat.
- `ends_with_end_anchor` mis-detection: a `$` following a group
  (`(?:ab)?$`, `(?:ab|cd)$`, `(?:ab)*$`, `(?:ab)+$`) was not recognized as an end
  anchor. The detection's single "visited" flag conflated cycle detection with
  DAG exploration, so branches converging on a shared `$` were misread as cycles
  (and looping groups were rejected outright). It now memoizes per-state results
  separately from on-stack cycle detection, and treats a loop back-edge as the
  neutral element for the "all paths end with `$`" check. This unblocks the
  end-anchor guards on other fast paths (e.g. `memchr`), so these patterns match
  correctly instead of being treated as bare literals.
- Leftmost-match bug for empty-accepting patterns in the DFA. A byte prefilter is
  not a sound leftmost filter when the pattern can match the empty string: an
  optional leading class was reduced to a single representative byte (`[0-9]?` →
  `0`), so `[0-9]?$` on `"2.2"` scanned only for `0`, found none, and returned the
  empty end match `(3,3)` instead of the leftmost `(2,3)`. `find_with_prefilter`
  now scans linearly for empty-accepting patterns (via `find_at`, which applies
  anchors and returns the longest match at each position), so greedy cases like
  `\w*` still match fully at position 0. Mandatory-class patterns keep their
  prefilters unchanged.
- Unsound DFA prefilter for patterns with an optional/alternation leading
  element. Branch first-byte collection dropped any branch whose leading atom was
  a named class (e.g. the `\d` arm of `,?\d`) and only kept one byte per branch,
  so `,?\d` built a `,`-only prefilter and reported no match on `"23"`. It now
  enumerates every possible first byte across all branches (including named
  classes) and falls back to a linear scan when a branch cannot be soundly
  enumerated, rather than building a partial filter that skips valid starts.
- `find()` returning wrong spans for class-plus-with-literal patterns whose
  leading class is not word-like. Its dedicated first-match helper extended
  greedily by a fixed word/email character set regardless of the actual class,
  so e.g. `\d?,` on `"b,.2-1"` matched `"b,.2-1"` instead of `","`, and
  `[+-]?\.\d` matched where it should not. `find()` now delegates the
  class-plus-with-literal case to the same class-aware logic `find_iter()` uses
  and takes the leftmost match, guaranteeing `find(x) == find_iter(x).next()`.
- `is_char_class_plus` mis-detecting a leading literal-plus before a class
  (`\.+\d`, `-+\d`, `@+\d{2}`) as a bare character-class-plus, so the DFA's
  `find_char_class_plus` fast path matched only the trailing class run (e.g.
  `\.+\d` on `"x..5"` returned `"5"` instead of `"..5"`). The detector now
  requires exactly one `Char` state (a single looping class with no other
  consuming structure); genuine `\d+`/`\w+`/`[a-z]+` are unaffected.
- Stack overflow at compile time for patterns with consecutive unbounded
  quantifiers over `.` (e.g. `.+.+`, `.*.*`, `^.+a{1,3}[+-]+$`). The
  greedy-prefix-with-suffix detector (`is_greedy_prefix_with_suffix` and its
  helpers) recursed through the NFA without cycle detection, so the loops from
  `+`/`*` caused unbounded recursion. The traversal now carries a `visited` set
  and terminates on cycles.
- `find()` returning no match for a quantified/optional literal group
  (`(?:ab)*`, `(?:ab)?`, `(?:ab)+`). The memchr fast path treated the pattern as a
  single fixed literal and scanned for one occurrence, so the empty-accepting
  forms wrongly returned `None` instead of the empty match. The memchr path is
  now disabled when the NFA contains any branching (`Split` or multi-target
  `Epsilon`), i.e. a quantifier/optional/alternation; plain fixed literals still
  use it.
- `.+` (and `^.+`, `^.+$`) matching empty text as `(0,0)` instead of `None`. The
  greedy dot-repeat fast path returned an empty match on empty input without
  honoring `+`'s minimum of one. The fast path now only applies to `.*`/`.+`
  (min 0 or 1) and returns `None` on empty text for `.+`; `.{2,}` and larger
  minimums fall through to the correct engine.
- `is_greedy_prefix_with_suffix` mis-detecting a single dot followed by a
  repeated group (`^.(?:,\d)*`) as `.*SUFFIX`, so `find` mishandled it (e.g.
  returned no match on `"3-"`). The detector now requires the `*`/`+` to actually
  repeat the dot — one of the loop's branches must lead back to the dot state;
  genuine `.*SUFFIX`/`.+SUFFIX` patterns are unaffected.
- `is_digit_sequence_with_separator` mis-detecting a pattern with a non-digit
  class between the digits and the separator (`\d{1,3}?[a-z]\.`), so the date
  fast path mishandled it. The detector now requires every `Char` state to be a
  digit class (a genuine digit sequence is only digits and separators); real
  date/number patterns (`\d{4}-\d{2}-\d{2}`, `\d+\.\d+`) are unaffected.
- Three more `find`/`find_iter` divergences, surfaced by a new crate-internal
  consistency check that asserts `find(x) == find_iter(x).next()` in default
  (leftmost) mode during the crate's own tests:
  - `search_first`'s "exact substring anywhere" shortcut returned a later exact
    match instead of an earlier fuzzy one (e.g. `(?:test){e<=1}` on
    `"best … test"` returned the exact `"test"`, not the leftmost fuzzy `"best"`).
    It is now used only for non-fuzzy patterns, where `str::find` is leftmost.
  - The per-operation-limit NFA fallback in `search_non_overlapping` returned
    overlapping, unsorted matches; it now returns leftmost, non-overlapping
    matches whose first equals `find`'s result.
  - Char-class edit restrictions (`{s<=1:[0-9]}`): `find` filtered only the first
    Bitap match and returned `None` if it failed the restriction; it now returns
    the first match that passes, matching `find_iter`.
- The greedy-prefix `.*SUFFIX` fast path treats `literals[0]` as the entire
  suffix, but fired for patterns whose suffix was more than one fixed literal —
  a class/group after the literal (`.+@.{2}`, `.+-\w{2}`), or a multi-segment
  literal from a group/`{n}` (`^.+-(?:ab)` → `["-","ab"]`, `^.+aa{2}` where
  `literals[0]="a"` but the suffix is `"aaa"`). These returned truncated or
  spurious matches. The suffix detector now requires exactly one literal unit
  (single `Char`/`FuzzyLiteral` reaching Accept via zero-width transitions only),
  and the fast path requires a single literal; everything else uses the DFA/NFA.
  Genuine `.*foo`/`.+\.txt`/`.+a` still use the fast path. (A dead, misconceived
  `PREFIX.*SUFFIX` block that only ever caught these buggy cases was removed.)
- `find()` returning spurious matches for a character-class-plus whose class has
  no named type. The fast path (`find_char_class_plus_first`) fell back to a WORD
  byte matcher when `get_char_class_type()` was `None`, i.e. for custom ranges
  (`[a-z]+?`, `[a-c]+`) and literal chars (`a+?`), so it matched the wrong bytes
  (`a+?` on `"xaay"` returned `"x"`; `[a-z]+?` on `"12ab"` returned `"1"`). It now
  requires a known named class (digit/word/whitespace and negations) before taking
  the byte path — mirroring `find_iter()`'s existing gate — and otherwise falls
  through to the class-aware engine. Genuine `\d+`/`\w+` are unaffected.
- The greedy-prefix `.*SUFFIX` fast path ignored the leading repeat's minimum, so
  `.+SUFFIX` (min 1) matched even when the only suffix occurrence sat at position
  0 with nothing for `.+` to consume (`.+@` on `"@13"` returned `(0,1)`; `.+-` on
  `"-…"` likewise). The path now records the leading dot-repeat's minimum and
  returns no match when the (rightmost) suffix is closer to the start than that
  minimum. `.*SUFFIX` (min 0) still matches with the suffix at position 0.
- `is_digit_sequence_with_separator` accepting a pattern whose separator is
  TRAILING (`\d{1,3}?\.` = digits then `.` with nothing after), so the date fast
  path claimed a match (`\d{1,3}?\.` on `".1 "` returned `(0,2)`). A genuine
  digit-sequence keeps every separator strictly BETWEEN digit groups, so the
  detector now also requires that no separator can reach the accept state without
  a following digit; real dates/numbers (`\d{4}-\d{2}-\d{2}`, `\d+\.\d+`) are
  unaffected.
- Fixed a further pre-existing `find_iter()` spurious match surfaced by the
  consistency work: `\d{1,3}?[a-z]\.` on `",,. -b."` (no digit or letter present)
  reported `(0,3)` under `find_iter()` while `find()` correctly reported no match;
  both now agree on no match under zero-edit matching.

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
