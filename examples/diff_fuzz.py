#!/usr/bin/env python3
"""Differential fuzzer: fuzzy-regex (Rust) vs mrab-regex (Python `regex`) oracle.

Generates random fuzzy patterns + texts, runs each through mrab-regex and the
Rust `diff_harness` example, and compares BOTH existence-of-match and the match
SPAN (byte offsets). The harness reports find() AND find_iter().next() per case.

Usage:
    cargo build --release --example diff_harness
    python3 examples/diff_fuzz.py [N_CASES] [SEED] [--spans]

Categories reported:
  * is_match divergences  -- mrab matched but rust didn't (or vice versa).
    mrab ZERO-WIDTH whole-pattern deletions are counted separately (fuzzy-regex
    deliberately declines those -- known policy, not a bug).
  * find != find_iter     -- INTERNAL inconsistency: the two Rust entry points
    return different spans for the same case. This is a real bug (they must
    agree); see memory `latent-fuzzy-find-bugs`.
  * find/iter vs mrab span -- engine-vs-mrab span differences. Many are the
    documented min-edit-vs-leftmost alignment divergence (informational); use
    --spans to list them and see which entry point tracks mrab.

Spans are compared in BYTES: mrab returns code-point indices, converted here via
the UTF-8 length of the text prefix so they line up with Rust byte offsets.
"""
import os
import random
import subprocess
import sys

try:
    import regex  # mrab-regex
except ImportError:
    sys.exit("pip install regex  (mrab-regex) is required")

HARNESS = os.path.join(
    os.path.dirname(__file__), "..", "target", "release", "examples", "diff_harness"
)

ALPHABET = "abc"          # pattern literal alphabet (small -> dense collisions)
TEXT_ALPHABET = "abcd"    # text alphabet (one extra symbol = noise)
UNICODE = "éü中\U0001f600"  # é ü 中 😀


def pct(s: str) -> str:
    """Percent-encode TAB, NEWLINE, % so the wire format is one case per line."""
    out = []
    for ch in s:
        if ch in "\t\n\r%":
            out.append("%%%02X" % ord(ch))
        else:
            out.append(ch)
    return "".join(out)


def rand_class(rng: random.Random) -> str:
    """A random character class fragment."""
    r = rng.random()
    if r < 0.3:
        return rng.choice([r"\d", r"\w", r"\s", "."])
    k = rng.randint(1, 3)
    body = "".join(rng.sample(ALPHABET + "d", min(k, 4)))
    neg = "^" if rng.random() < 0.3 else ""
    if rng.random() < 0.3:
        return "[a-c]"
    return f"[{neg}{body}]"


def rand_core(rng: random.Random, depth: int = 0) -> str:
    """A random non-fuzzy regex fragment over ALPHABET."""
    kind = rng.random()
    if kind < 0.38:  # literal run
        n = rng.randint(1, 4)
        return "".join(rng.choice(ALPHABET) for _ in range(n))
    elif kind < 0.55:  # char class
        return rand_class(rng)
    elif kind < 0.70:  # alternation of two/three fragments (sometimes identical)
        n = rng.randint(2, 3)
        if rng.random() < 0.3:  # identical branches (regression guard)
            b = "".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 3)))
            parts = [b] * n
        else:
            parts = ["".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 3)))
                     for _ in range(n)]
        return "(?:" + "|".join(parts) + ")"
    elif kind < 0.85:  # quantified char/class
        base = rng.choice(ALPHABET) if rng.random() < 0.6 else rand_class(rng)
        q = rng.choice(["+", "*", "?", "{1,3}", "{2}", "{0,2}", "{2,3}"])
        return base + q
    elif kind < 0.95 and depth < 2:  # concatenation of 2-3 sub-pieces
        n = rng.randint(2, 3)
        return "".join(rand_core(rng, depth + 1) for _ in range(n))
    else:  # unicode literal
        return rng.choice(UNICODE)


def rand_fuzzy(rng: random.Random) -> str:
    """A random `{...}` fuzzy spec (or empty)."""
    r = rng.random()
    if r < 0.22:
        return ""
    if r < 0.30:  # combined individual + total
        i = rng.randint(0, 2)
        e = rng.randint(i, i + 2)
        t = rng.choice(["i", "d", "s"])
        return "{%s<=%d,e<=%d}" % (t, i, e)
    if r < 0.36:  # exclusive bound
        k = rng.randint(1, 3)
        return "{e<%d}" % k
    if r < 0.42:  # range
        lo = rng.randint(0, 1)
        hi = rng.randint(lo + 1, lo + 3)
        return "{%d<=e<=%d}" % (lo, hi)
    k = rng.randint(0, 3)
    t = rng.choice(["e", "i", "d", "s"])
    return "{%s<=%d}" % (t, k)


def rand_pattern(rng: random.Random) -> str:
    core = rand_core(rng)
    fuzzy = rand_fuzzy(rng)
    if fuzzy and not core.startswith("(?:"):
        core = f"(?:{core})"
    pat = core + fuzzy
    # occasionally anchor
    a = rng.random()
    if a < 0.10:
        pat = "^" + pat
    elif a < 0.18:
        pat = pat + "$"
    elif a < 0.22:
        pat = "^" + pat + "$"
    return pat


def rand_text(rng: random.Random, pat_literal: str) -> str:
    n = rng.randint(0, 18)
    alpha = TEXT_ALPHABET + (UNICODE if rng.random() < 0.1 else "")
    base = "".join(rng.choice(alpha) for _ in range(n))
    if pat_literal and rng.random() < 0.5:
        s = list(pat_literal)
        for _ in range(rng.randint(0, 2)):
            if not s:
                break
            op = rng.randint(0, 2)
            i = rng.randrange(len(s))
            if op == 0:
                s[i] = rng.choice(TEXT_ALPHABET)
            elif op == 1:
                s.pop(i)
            else:
                s.insert(i, rng.choice(TEXT_ALPHABET))
        pos = rng.randint(0, len(base))
        base = base[:pos] + "".join(s) + base[pos:]
    return base


def mrab_search(pat: str, text: str):
    """mrab oracle result, or None if mrab can't compile/run.

    Returns a dict:
        matched: bool
        zero_width: bool               (whole-pattern deletion)
        span: (bstart, bend) | None    byte offsets, comparable to Rust
        counts: (subs, ins, dels) | None
    """
    try:
        re = regex.compile(pat)
    except Exception:
        return None
    try:
        m = re.search(text)
    except Exception:
        return None
    if m is None:
        return {"matched": False, "zero_width": False, "span": None, "counts": None}
    cs, ce = m.start(), m.end()
    # mrab indices are code points; convert to bytes for Rust comparison.
    bstart = len(text[:cs].encode("utf-8"))
    bend = len(text[:ce].encode("utf-8"))
    return {
        "matched": True,
        "zero_width": ce == cs,
        "span": (bstart, bend),
        "counts": tuple(m.fuzzy_counts),  # (subs, ins, dels)
    }


def parse_rust(tok: str):
    """Parse one Rust result half (`N` or `s,e,su,i,d`) into None | (span, counts)."""
    if tok == "N":
        return None
    s, e, su, i, d = (int(x) for x in tok.split(","))
    return ((s, e), (su, i, d))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    show_spans = "--spans" in sys.argv[1:]
    n = int(args[0]) if len(args) > 0 else 20000
    seed = int(args[1]) if len(args) > 1 else 12345
    rng = random.Random(seed)

    if not os.path.exists(HARNESS):
        sys.exit(f"harness not built: {HARNESS}\n  cargo build --release --example diff_harness")

    cases = []          # (pat, text)
    oracle = []         # mrab dict
    for _ in range(n):
        pat = rand_pattern(rng)
        lit = "".join(ch for ch in pat if ch in ALPHABET)
        text = rand_text(rng, lit)
        m = mrab_search(pat, text)
        if m is None:
            continue
        cases.append((pat, text))
        oracle.append(m)

    payload = "".join(f"{pct(p)}\t{pct(t)}\n" for p, t in cases)
    proc = subprocess.run([HARNESS], input=payload, capture_output=True, text=True)
    rust_lines = proc.stdout.splitlines()
    if len(rust_lines) != len(cases):
        print(f"WARN: harness returned {len(rust_lines)} lines for {len(cases)} cases")

    exist_div = []    # is_match existence divergences (real)
    zerowidth = 0     # mrab zero-width match, rust declines (known policy)
    panics = []
    compile_errs = []
    find_ne_iter = []       # find() != find_iter().next() -- internal bug
    both_matched = 0        # mrab matched AND rust find() matched
    find_eq_mrab = 0        # find() span == mrab span
    iter_eq_mrab = 0        # find_iter() span == mrab span
    span_div = []           # find span != mrab span (informational)

    for (pat, text), m, got in zip(cases, oracle, rust_lines):
        if got == "P":
            panics.append((pat, text))
            continue
        if got == "E":
            compile_errs.append((pat, text))
            continue
        find_tok, iter_tok = got.split("|", 1)
        find = parse_rust(find_tok)
        it = parse_rust(iter_tok)
        find_span = find[0] if find else None
        iter_span = it[0] if it else None

        # Internal consistency: find() must equal find_iter().next() (span+counts).
        if find != it:
            find_ne_iter.append((pat, text, find, it))

        # Existence vs mrab.
        rust_matched = find is not None
        if rust_matched != m["matched"]:
            if m["matched"] and not rust_matched and m["zero_width"]:
                zerowidth += 1
            else:
                exist_div.append((pat, text, m["matched"], rust_matched))
            continue

        # Both agree on existence; if both matched, compare spans to mrab.
        if m["matched"] and rust_matched:
            both_matched += 1
            if find_span == m["span"]:
                find_eq_mrab += 1
            if iter_span == m["span"]:
                iter_eq_mrab += 1
            if find_span != m["span"]:
                span_div.append((pat, text, m["span"], find_span, iter_span))

    def pctf(x, tot):
        return f"{x} ({100.0 * x / tot:.1f}%)" if tot else str(x)

    print(f"cases compared: {len(cases)}  (seed={seed})")
    print(f"  rust panics:                    {len(panics)}")
    print(f"  is_match divergences (real):    {len(exist_div)}")
    print(f"  zero-width policy (expected):   {zerowidth}")
    print(f"  compile divergences:            {len(compile_errs)}")
    print(f"  find() != find_iter() (BUG):    {len(find_ne_iter)}")
    print(f"  both matched (span comparable):  {both_matched}")
    print(f"    find()      span == mrab:      {pctf(find_eq_mrab, both_matched)}")
    print(f"    find_iter() span == mrab:      {pctf(iter_eq_mrab, both_matched)}")
    print(f"    find()      span != mrab:      {len(span_div)}")

    def show(title, rows, fmt, limit=30):
        if not rows:
            return
        print(f"\n=== {title} (showing up to {limit}) ===")
        for row in rows[:limit]:
            print(fmt(row))

    show("PANICS", panics, lambda r: f"  pat={r[0]!r:34} text={r[1]!r}")
    show("is_match DIVERGENCES", exist_div,
         lambda r: f"  pat={r[0]!r:34} text={r[1]!r:22} mrab={r[2]} rust={r[3]}")
    show("find() != find_iter() [BUG]", find_ne_iter,
         lambda r: f"  pat={r[0]!r:32} text={r[1]!r:20} find={r[2]} iter={r[3]}")
    show("COMPILE DIVERGENCES", compile_errs,
         lambda r: f"  pat={r[0]!r:34} text={r[1]!r}")
    if show_spans:
        show("SPAN vs mrab (find != mrab)", span_div,
             lambda r: f"  pat={r[0]!r:30} text={r[1]!r:18} mrab={r[2]} find={r[3]} iter={r[4]}",
             limit=60)

    # Panics and existence divergences are hard bugs. find!=iter is also a bug but
    # is currently known/open, so it is reported without failing the run (flip to
    # include `find_ne_iter` once fixed).
    sys.exit(1 if (panics or exist_div) else 0)


if __name__ == "__main__":
    main()
