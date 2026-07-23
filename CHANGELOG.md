# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Performance

- `find` on fuzzy patterns that match many times is no longer O(all matches).
  The general fallback previously computed every non-overlapping match via
  `find_iter` and discarded all but the first; it now stops at the first match
  using the matcher's `find_n(_, 1)` (which shares `find_all`'s scan via a new
  `find_up_to(text, limit)`), guarded so it only applies when `find_iter` would
  use that same general path. E.g. `(?:\w+){e<=1} (?:\w+){e<=1}` drops from
  ~215µs to ~34µs (~6x); a lone fuzzy `(?:[a-z]+){e<=1}` ~20x. Verified
  identical to `find_iter().next()` by the 50k-case consistency proptest.

### Added

- Fuzzy quantifier on a recursion reference, e.g. `(?R){e<=2}` / `(?0){e}` /
  `(?1){e<=1}`. The fuzziness is threaded onto the recursion states and enforced
  by the backtracker as a total-edit cap on the recursive sub-match (an
  unbounded `{e}` places no cap, so it behaves like plain `(?R)`); non-fuzzy
  recursion is unchanged. Unblocks mrab corpus L3804 — the last non-divergence
  entry, so every unsupported-feature gap in the corpus is now closed.

- `\m` (start-of-word) and `\M` (end-of-word) boundary markers — the directional
  halves of `\b`. `\m` matches a non-word→word transition (or string start
  before a word char); `\M` matches a word→non-word transition (or string end
  after a word char). They compose with fuzziness and lookaround. Also accept
  the mrab `(?V0)`/`(?V1)` version flags (no behavioural effect). Together these
  unblock mrab corpus L654, L657.

- Coefficient-less cost expressions in fuzziness, e.g. `{i+d+s<=N}`,
  `{s+i+d<=N}`, `{i+d<=N}`. Previously the weighted-cost syntax required an
  explicit coefficient on every term (`{1i+1d+1s<=N}`); a bare `i`/`d`/`s`/`t`
  now defaults to coefficient 1. Caps the total weighted edit cost (for unit
  coefficients, `{i+d+s<=N}` is equivalent to `{e<=N}`), and composes with
  per-op caps (`{i<=4,d<=4,s<=4,i+d+s<=8}`). Unblocks mrab corpus L2798, L4181.

- Recursive patterns: `(?R)`/`(?0)` (whole pattern), `(?1)`/`(?2)`/… (numbered
  group), and `(?&name)`/`(?P>name)` (named group). Recursion is executed by the
  backtracking engine as a subroutine call stack integrated into the single
  backtracking search — so choices made inside a recursive call are revisited
  when a later part of the match fails, and patterns like balanced delimiters
  (`\((?:[^()]|(?R))*\)`) match correctly, including deep nesting. Left
  recursion (a call that makes no progress) is detected and fails rather than
  looping. Previously recursion parsed but was compiled to a dead branch that
  never matched. Unblocks mrab corpus L3801.

- `(?f)` full case folding (only effective with `(?i)`): matches Unicode
  characters with multi-character case foldings against their expansions,
  bidirectionally — `(?fi)\N{LATIN SMALL LETTER SHARP S}` matches "ss"/"SS"/"ß",
  and `(?fi)ss` matches "ß"; likewise the ligatures (ﬀ↔"ff", etc.). Implemented
  as an opt-in AST rewrite (a fold-expanding character becomes `(?:char|fold)`
  and a literal run equal to a fold gains the collapsed character as an
  alternative), so it composes with fuzziness and does not affect patterns that
  do not set `(?f)`. A pattern character never matches a partial expansion (a
  bare "s" does not match "ß"). Adds a dependency on `caseless`. Also adds
  `FuzzyRegexBuilder`-level parsing of the flag.

- `\N{...}` named-Unicode escapes: `\N{NAME}` resolves a Unicode character name
  (e.g. `\N{LATIN SMALL LETTER SHARP S}`, `\N{BULLET}`) via the `unicode-names2`
  database, and `\N{U+XXXX}` resolves a codepoint. Works anywhere a literal
  character does — in sequences, character classes, with quantifiers and
  fuzziness. Unknown names, malformed forms, and out-of-range codepoints are
  compile errors. Unblocks the mrab corpus sharp-s cases (L4448–L4496). Adds a
  dependency on `unicode-names2` (its table generator is a build-only dep).

- Forward references — a backreference to a capture group defined later in the
  pattern (`\1(a)`, `\k<x>(?P<x>a)`, and the `(?r)` reverse-mode forms). A
  pre-scan counts groups and collects names before parsing, so the reference
  resolves against the full group set rather than only groups seen so far; a
  reference to a group that never exists is still a compile error. When the
  referenced group has not captured at the point the reference is reached, it
  matches the empty string (matching Python's `regex` module). Unblocks mrab
  corpus cases L2762, L4441, L4445, L4486, L4488.

- Named backreferences: `\k<name>`, `\k{name}`, and Python-style `(?P=name)`.
  The name is resolved to the group's index at parse time and matched via the
  existing backreference machinery (numeric `\1`…`\9` were already supported),
  so named backreferences also accept a fuzziness suffix (`\k<w>{e<=1}`). Unknown
  or malformed names are a compile error.

- mrab-style fuzzy quantifiers (`{e<=1}`, `{i<=1,d<=2}`, `{s<=1}`, …) on unnamed
  capturing groups, e.g. `(abc){e<=1}`. Previously only non-capturing
  `(?:abc){e<=1}` and named `(?P<n>abc){e<=1}` groups accepted the syntax; the
  unnamed capturing path recognised only the `~N` form and rejected `{e}` with
  "invalid quantifier: expected number". Unnamed captures now behave exactly like
  named captures (same `apply_group_fuzziness` lowering), populate the group, and
  compose with `(?r)`. Unblocks mrab corpus cases L3786/L3788/L3790
  (`(?r)(x{6}){e<=1}`) and L4330 (`(\L<foo>){e<=5}`).

- `(?r)` reverse-matching inline flag (and `FuzzyRegexBuilder::reverse`). When
  set, the engine searches from the end of the text toward the start: `find`
  returns the rightmost match, `find_iter` yields matches right-to-left, and
  `captures` returns the rightmost match's groups. Match existence (`is_match`)
  is direction-independent. The flag composes with the others (e.g. `(?er)`,
  `(?ri)`). Reverse search reuses the existing right-to-left DFA scan
  (`Dfa::find_rev`) and falls back to enumerating forward matches and taking the
  rightmost for fuzzy/lookaround/capture patterns. This unblocks 10 previously
  compile-failing mrab corpus cases (reverse + word lists / lookaround).

- `FuzzyRegex::find_all_hardened` — find all non-overlapping matches in a single
  linear-time DFA pass. For patterns like `.*a|b` on a long run of `b`s, the
  usual "find, advance, repeat" loop (`find_iter`) is O(n²) because each match's
  longest extent needs an independent look-ahead; the hardened scan is O(n) (it
  tracks all live threads deduplicated by DFA state, records every accept, then
  selects leftmost-longest non-overlapping). Results are identical to
  `find_iter` (verified by a property test against `find_all` over 21 patterns ×
  200 random inputs each). ~1.6M chars in ~120 ms vs. quadratic blowup. Patterns
  that need the NFA fall back to `find_iter`. The internal `Dfa::find_all_hardened`
  was also **fixed**: it previously emitted the scan-stop position instead of the
  accepting end (so `.*a|b` on `"bbbb"` returned one bogus `(0,4)` instead of four
  `"b"` matches), and its outer loop re-scanned from every position (O(n²)); the
  single-pass rewrite is both correct and linear.
- Aho-Corasick fast path for large `\L<name>` word lists, behind the new
  default-on `word-list-ac` feature (uses the
  [`fuzzy-aho-corasick`](https://github.com/kakserpom/fuzzy-aho-corasick-rs)
  crate). A *pure word-list* pattern — a single `\L<name>` (optionally fuzzy)
  wrapped only in `^`/`$` anchors and/or `\b` word boundaries, with no capture
  groups — whose resolved list exceeds a threshold is matched by a single
  Aho-Corasick pass instead of being expanded into an NFA alternation of every
  word (which is slow to build and scan for large lists). Selection is
  leftmost-longest and non-overlapping, and the surrounding anchors/boundaries
  are enforced against the full text, so results match the NFA alternation
  exactly (verified in tests). Small lists and non-pure patterns keep using the
  NFA. Disable the feature for a lighter dependency tree if you do not use large
  named lists. A 5000-word list compiles in a few milliseconds and matches in
  microseconds. The size threshold is configurable via
  `FuzzyRegexBuilder::word_list_ac_threshold(n)` (default
  `DEFAULT_WORD_LIST_AC_THRESHOLD` = 64); set it very high to keep every list on
  the NFA, or low to favor the AC path sooner. `benches/fuzzy_benchmarks.rs`
  includes `word_list_match` / `word_list_compile` groups comparing the two paths
  across list sizes (match is ~60× faster at 128 words, growing past 200× for
  thousands of words, since the NFA alternation scans in O(list × text) while the
  automaton is effectively constant).
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

- The exact-alternation Aho-Corasick fast path (behind the `fuzzy-aho-corasick`
  feature) used `MatchKind::LeftmostFirst`, which returns the earliest-listed
  branch rather than the longest, so it disagreed with the engine's
  leftmost-longest semantics — e.g. `(?:cat|cats)` on `"cats"` returned `"cat"`
  with the feature enabled instead of `"cats"`. It now uses
  `MatchKind::LeftmostLongest`, matching the NFA. (Default builds were
  unaffected; the feature is off by default.)
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
- Full, uniform `\L<name>` named-list support. Previously named lists were
  matched by a bespoke substring/Levenshtein matcher used only by `find`/
  `find_iter`, so it ignored surrounding anchors (`\b\L<w>\b` matched inside a
  word), produced no sub-captures, could not handle `\L` embedded in a larger
  pattern, and left `captures`/`find_at` returning no match. `set_word_list` now
  expands each resolved `\L<name>` into a real NFA alternation of its words
  (inside the reference's fuzzy group) and rebuilds the compiled automaton, so
  the named list is matched by the normal engine on every path. As a result:
  surrounding anchors are honored, capture groups around/near `\L` work, `\L`
  composes inside larger patterns (`(\w+)=\L<v>`), fuzzy edits apply to the
  alternation (`\L<w>{e<=1}`), and `find`/`find_iter`/`captures`/`find_at` are
  all consistent. `clone` now also preserves resolved word lists (it previously
  re-compiled from the pattern and dropped them). Note: expanding a very large
  word list produces a correspondingly large alternation; matching stays correct
  but is no longer served by the previous bespoke fast path.
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
