#!/usr/bin/env python3
"""Differential fuzzer: fuzzy-regex (Rust) vs mrab-regex (Python `regex`) oracle.

Generates random fuzzy patterns + texts, computes the existence-of-match answer
with mrab-regex, then feeds the same cases to the Rust `diff_harness` example and
flags any case where the two disagree.

Usage:
    cargo build --release --example diff_harness
    python3 examples/diff_fuzz.py [N_CASES] [SEED]

Only `is_match` (does any fuzzy match exist) is compared -- that existence
question is semantically unambiguous across engines, unlike match spans.

Divergences where mrab returns a ZERO-WIDTH match (it deletes the whole pattern
within the deletion budget) are reported separately: fuzzy-regex deliberately
declines zero-width fuzzy matches, so those are a known semantic difference, not
a bug. Everything else is a real divergence worth investigating.
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
    """Return (matched: bool, zero_width: bool) or None if mrab can't compile."""
    try:
        re = regex.compile(pat)
    except Exception:
        return None
    try:
        m = re.search(text)
    except Exception:
        return None
    if m is None:
        return (False, False)
    return (True, m.end() == m.start())


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 20000
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 12345
    rng = random.Random(seed)

    if not os.path.exists(HARNESS):
        sys.exit(f"harness not built: {HARNESS}\n  cargo build --release --example diff_harness")

    cases = []          # (pat, text)
    oracle = []         # (matched, zero_width)
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

    real = []         # genuine divergences
    zerowidth = 0     # mrab zero-width match, rust declines (known policy)
    panics = []
    compile_errs = []
    for (pat, text), (exp, zw), got in zip(cases, oracle, rust_lines):
        if got == "P":
            panics.append((pat, text))
        elif got == "E":
            compile_errs.append((pat, text))
        else:
            rust = got == "1"
            if rust != exp:
                if exp and not rust and zw:
                    zerowidth += 1
                else:
                    real.append((pat, text, exp, rust))

    print(f"cases compared: {len(cases)}  (seed={seed})")
    print(f"  REAL divergences:  {len(real)}")
    print(f"  zero-width policy: {zerowidth}  (mrab deletes whole pattern; expected)")
    print(f"  rust panics:       {len(panics)}")
    print(f"  rust compile-errs (mrab accepted): {len(compile_errs)}")

    def show(title, rows, fmt):
        if not rows:
            return
        print(f"\n=== {title} (showing up to 30) ===")
        for row in rows[:30]:
            print(fmt(row))

    show("PANICS", panics, lambda r: f"  pat={r[0]!r:34} text={r[1]!r}")
    show("REAL DIVERGENCES", real,
         lambda r: f"  pat={r[0]!r:34} text={r[1]!r:22} mrab={r[2]} rust={r[3]}")
    show("COMPILE DIVERGENCES", compile_errs,
         lambda r: f"  pat={r[0]!r:34} text={r[1]!r}")

    sys.exit(1 if (panics or real) else 0)


if __name__ == "__main__":
    main()
