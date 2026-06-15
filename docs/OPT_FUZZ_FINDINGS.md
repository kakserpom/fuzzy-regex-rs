# Optimization & Differential-Fuzzing Findings (2026-06-15)

Work session: explore engine optimizations, take inspiration from `resharp`,
compare performance with mrab-regex (Python `regex`), and fuzz.

## 1. Differential fuzzer vs mrab-regex (NEW)

Reusable harness comparing existence-of-match against the Python `regex`
(mrab) module as an oracle:

- `examples/diff_harness.rs` — stdin-driven Rust driver (`pattern\ttext`,
  percent-encoded → `1`/`0`/`E`/`P`).
- `examples/diff_fuzz.py` — generator + oracle + comparator.

Run:
```
cargo build --release --example diff_harness
python3 examples/diff_fuzz.py 20000 12345
```
Exits non-zero on any panic or match divergence (CI-friendly).

First run (seed 12345, 20 000 cases) found **738 divergences**. Categorized:

| count | op       | direction      | meaning |
|------:|----------|----------------|---------|
| 513   | `i<=k`   | mrab=0 rust=1  | insertion-only too permissive |
|  79   | `s<=k`   | mrab=0 rust=1  | substitution-only too permissive |
|  67   | `e<=k`   | mrab=1 rust=0  | rust too strict (short/empty deletion cases) |
|  29   | `s<=k`   | mrab=1 rust=0  | |
|  22   | `d<=k`   | mrab=1 rust=0  | |
|  16   | `d<=k`   | mrab=0 rust=1  | |
|   6   | exact    | mrab=1 rust=0  | **FIXED** — see §2 |

## 2. Bug fixed: identical-branch alternation (`bc|bc`)

`bc|bc`, `cat|cat`, `bcd|bcd` etc. returned **no match**. Root cause:
`api/regex.rs` "repetition fast path" assumed any pattern yielding ≥2
*identical* literals is `(?:literal){N}`, flattening `bc|bc` into the string
`"bcbc"`. A top-level alternation of identical branches also yields N identical
literals. Fix: guard the fast path with `&& !is_simple_alternation`
(`src/api/regex.rs`). Real repetitions (`(?:bc){2}`) are unaffected (they are
concatenations, not `Split` states). All 6 exact divergences eliminated; full
test suite stays green (21/21).

## 3. Open issue: per-operation limits `{i<=k}` / `{d<=k}` / `{s<=k}`

mrab rule (derived empirically): **if any individual op is specified, every
unspecified op defaults to 0**; `e<=` only caps the total and never raises an
unspecified op above 0. Each individual cap is hard-enforced.

fuzzy-regex diverges in two places:
1. `MrabFuzziness::to_limits` (`src/parser/ast.rs`) leaves unspecified ops as
   `None`, which the engine treats as "unlimited up to the total budget"
   (the total is already `sum(i,d,s)` — `fuzzy_bridge.rs:120`).
2. The Bitap path attributes edits to op types only approximately, so even an
   **explicit** `s<=0`/`d<=0` leaks (`(?:test){i<=2,d<=0,s<=0}` matches `txst`,
   a 1-substitution input).

**FIXED.** Two changes:
1. `MrabFuzziness::to_limits` (`src/parser/ast.rs`): when individual ops are
   specified *without* an explicit total (`e<=`/cost), default the unspecified
   ops to `0`. (When an explicit `e<=` budget is given it governs the
   unspecified ops, preserving intent like `{t<=1,e<=2}`.)
2. `FuzzyBridge::new` (`src/engine/fuzzy_bridge.rs`): Bitap only enforces the
   total edit budget (it never reads the per-op caps), so the bridge now passes
   `None` for the Bitap matcher when a per-op cap is *binding* (`Some(x)` with
   `x < max_edits`). All search paths already fall back to the
   Damerau-Levenshtein NFA when Bitap is absent (the same path used for >64-char
   patterns), and that NFA tracks per-op counts and enforces them exactly.
   Exception: patterns with an edit-character restriction (`{s<=1:[a-z]}`) keep
   Bitap, because the matcher validates those via the Bitap path.

Result: differential divergences dropped **738 → 228** (seed 12345, 20 000
cases); over-permissive (rust matches, mrab doesn't) dropped **608 → 13**.
Generalizes across seeds (777→217, 2024→246), zero panics, zero compile
divergences. Full suite green (786 tests), clippy clean.

### Two more fixes (quantifier-in-group + fuzzy-class substitution)

3. **Quantifier inside a fuzzy group** (`(?:b{2}){e<=1}`) lowered to N
   independent fuzzy copies of the literal (each with the full edit budget), so
   `(?:b{2}){e<=1}` behaved like `b{e<=1}b{e<=1}` (== e<=2). Fixed in
   `lower_with_detailed_limits` (`src/ir/hir.rs`): a fixed/bounded repeat of a
   single literal/char is now folded into one fuzzy literal of the repeated text
   (`{N}` → one literal; `{N,M}` → an alternation), so the edit budget is shared.
   Eliminated the last over-permissive divergences (→ **0**).
4. **Standalone fuzzy character class via substitution** (`(?:[a]){s<=1}` on
   `"d"`) returned no match. `Nfa::first_char_class` (`src/ir/nfa.rs`) used a
   fuzzy first char as an exact prefilter whenever deletions were 0, ignoring
   substitutions — so the position scan skipped every char not literally in the
   class. Fixed: the class is used as a prefilter only when neither substitution
   nor deletion is possible at the first char (an insertion budget is still
   fine). Eliminated all 57 non-empty class misses.

## 6. Broadened fuzzer + further fixes

`diff_fuzz.py` was extended (anchors, unicode, character classes with
ranges/negation/escapes, multi-piece concatenations, ranges & exclusive bounds,
combined individual+total specs) and now classifies mrab zero-width
whole-pattern deletions separately so real bugs surface. This found:

5. **UTF-8 panic** (~90 inputs/run): three find loops (`find_all`, `find_n`,
   `find_all_with_literal_guide` in `matcher.rs`) advanced `pos + 1` (one byte)
   after a zero-width match, landing mid-char → `text[pos..]` panicked. Now use
   the char-boundary-safe advance. **Panics → 0.**
6. **mrab per-op/cost rule refinement** (`ast.rs`): unspecified i/d/s default to
   0 whenever any of i/d/s is named — *including* with an explicit `e<=` (which
   only caps the total). Transposition (`t`, a fuzzy-regex extension) and cost
   constraints suppress the defaulting. Eliminated the `{i<=k,e<=m}` over-match
   class.
7. **Alternation with duplicate branches** (`ab|a|ab`, `c|bc|bc`, `^(?:bb|bb)`):
   a `Char` branch isn't extracted as a literal, so `is_simple_alternation` was
   false and the repetition fast path still flattened the identical literals to
   `literal.repeat(N)`. The fast path now requires a `Split`-free NFA (a genuine
   concatenation), which subsumes the earlier alternation guard.
8. (also) test `test_bestmatch_finds_best` updated: it asserted a
   2-substitution match while naming only i/d (which forces s=0 per mrab); added
   `s<=3` so the intended match is reachable.

## 7. Final state & remaining divergences

After all fixes: **0 panics, 0 over-permissive for core mrab semantics**, 786
tests + 21 doctests green, clippy clean. Across seeds 12345/777/2024/555
(20 000 cases each) ~110–140 REAL divergences remain, in three classes — none a
crash, all niche:

- **Zero-width whole-pattern deletion** (mrab emits empty match, fuzzy-regex
  declines) — semantic policy choice; would match at every position of any text.
- **Multi-piece fuzzy groups** (`(?:b{1,3}cabcab){d<=3}`) — over-match: the
  group's edit budget should be *shared* across the concatenated `FuzzyLiteral`
  pieces, but each piece currently honors the full budget independently. Fixing
  needs `FuzzyLiteral` to enforce a global remaining budget
  (`limit - thread.edits_so_far`); touches the core NFA + fuzzy-cache path.
- **Fuzzy `FuzzyChar` insertion** (`^(?:[cd]){i<=1}$` on `"dz"`) — under-match:
  the `FuzzyChar` NFA step (`matcher.rs`) implements substitution and deletion
  but not insertion; trailing insertions before `$` are especially fiddly.

Both engine-level classes are deep changes to perf-critical matching for
uncommon patterns; deferred pending a priority/risk decision.

## 4. Performance vs mrab-regex (existing harness)

`examples/mrab_bench.py` vs the equivalent Rust harness
(`benchmarks/bench_vs_mrab.rs`). us/iter, lower is better:

| test                              | mrab   | fuzzy-regex | speedup |
|-----------------------------------|-------:|------------:|--------:|
| `quick` e≤1 (short)               |  0.83  |   0.08      | ~10×    |
| `Lorem` e≤2 (medium)              |  0.35  |   0.04      | ~9×     |
| `Lorem` e≤2 (long 3.8 KB)         |  0.34  |   0.04      | ~8×     |
| `quick` s≤1 (short)               |  0.66  |   0.04      | ~16×    |
| `xyzzy` e≤1 no-match (short)      |  5.98  |   0.44      | ~14×    |
| `xyzzy` e≤1 no-match (medium)     | 24.65  |   1.77      | ~14×    |
| `ACGTACGT` e≤2 in DNA (1 kb)      |  0.33  |   0.09      | ~4×     |

fuzzy-regex is already 4–16× faster on these. The slowest path is full-scan
no-match (`1.77 µs` on 191 B). See `PERFORMANCE.md` for the vs-`regex`-crate
picture (we win on `\d+`, `\b\w+\b`, classes; lose on exact-literal memchr,
`$`-anchors, `(?:x){N}` repetition).

## 5. resharp optimization roadmap (inspiration)

`resharp` (https://github.com/ieviev/resharp) is a symbolic-derivative RE#
engine. Pulled to `resharp/`. Techniques portable to fuzzy-regex, by ROI:

- **Byte-frequency skip acceleration** — precomputed rarity table chooses the
  rarest pattern byte to SIMD-skip on; only activates when profitable. (resharp
  `simd/byte_freq.rs`.) We already have Teddy/`BYTE_FREQ`; adopt the
  profitability gate. *High ROI on large no-match haystacks (our slow path).*
- **Compiled-automaton serialization** (`to_bytes`/`from_bytes`) — 47–53× on
  repeated pattern use; cache compiled NFA/DFA. *High ROI if patterns reused.*
- **Bounded-repeat (BDFA) explicit count states** for `\d{3,5}`-style bounds —
  linear, not exponential. *Medium ROI.*
- **Case-folding compiled into transitions** (no slow fallback for `(?i)`).
  *Medium ROI.*
- **Minterms / partition refinement** — large state-table savings but needs a
  symbolic-algebra layer; *low feasibility* without a rewrite.

See `docs/RE_SHARP_RESEARCH.md` and `RE-PLAN.md` (literal-prefix scanning,
full-DFA precompile, Hopcroft minimization, start-set inference) for prior notes
that align with the above.
