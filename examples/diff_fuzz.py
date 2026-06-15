#!/usr/bin/env python3
"""Differential fuzzer: fuzzy-regex (Rust) vs mrab-regex (Python `regex`) oracle.

Generates random fuzzy patterns + texts, computes the existence-of-match answer
with mrab-regex, then feeds the same cases to the Rust `diff_harness` example and
flags any case where the two disagree.

Usage:
    # build the harness once:
    cargo build --release --example diff_harness
    python3 examples/diff_fuzz.py [N_CASES] [SEED]

Only `is_match` (does any fuzzy match exist) is compared -- that existence
question is semantically unambiguous across engines, unlike match spans.
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


def pct(s: str) -> str:
    """Percent-encode TAB, NEWLINE, % so the wire format is one case per line."""
    out = []
    for ch in s:
        if ch in "\t\n\r%":
            out.append("%%%02X" % ord(ch))
        else:
            out.append(ch)
    return "".join(out)


def rand_core(rng: random.Random) -> str:
    """A random non-fuzzy regex fragment over ALPHABET."""
    kind = rng.random()
    if kind < 0.45:  # literal run
        n = rng.randint(1, 4)
        return "".join(rng.choice(ALPHABET) for _ in range(n))
    elif kind < 0.65:  # char class
        k = rng.randint(1, 3)
        return "[" + "".join(rng.sample(ALPHABET, k)) + "]"
    elif kind < 0.80:  # alternation of two literals
        a = "".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 3)))
        b = "".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 3)))
        return f"(?:{a}|{b})"
    else:  # quantified single char
        c = rng.choice(ALPHABET)
        q = rng.choice(["+", "*", "?", "{1,3}", "{2}"])
        return c + q


def rand_fuzzy(rng: random.Random) -> str:
    """A random `{...}` fuzzy spec (or empty)."""
    r = rng.random()
    if r < 0.25:
        return ""
    k = rng.randint(0, 3)
    t = rng.choice(["e", "i", "d", "s"])
    return "{%s<=%d}" % (t, k)


def rand_pattern(rng: random.Random) -> str:
    core = rand_core(rng)
    fuzzy = rand_fuzzy(rng)
    if fuzzy and not core.startswith("(?:"):
        core = f"(?:{core})"
    return core + fuzzy


def rand_text(rng: random.Random, pat_literal: str) -> str:
    n = rng.randint(0, 16)
    base = "".join(rng.choice(TEXT_ALPHABET) for _ in range(n))
    if pat_literal and rng.random() < 0.5:
        # splice a possibly-mutated copy of a literal into the text
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


def mrab_match(pat: str, text: str):
    """Return True/False, or None if mrab can't compile the pattern."""
    try:
        re = regex.compile(pat)
    except Exception:
        return None
    try:
        return re.search(text) is not None
    except Exception:
        return None


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 20000
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 12345
    rng = random.Random(seed)

    if not os.path.exists(HARNESS):
        sys.exit(f"harness not built: {HARNESS}\n  cargo build --release --example diff_harness")

    cases = []          # (pat, text)
    oracle = []         # True/False/None
    for _ in range(n):
        pat = rand_pattern(rng)
        # extract a literal-ish seed for the text from the core
        lit = "".join(ch for ch in pat if ch in ALPHABET)
        text = rand_text(rng, lit)
        m = mrab_match(pat, text)
        if m is None:
            continue  # skip patterns mrab rejects
        cases.append((pat, text))
        oracle.append(m)

    # Feed all cases to the Rust harness at once.
    payload = "".join(f"{pct(p)}\t{pct(t)}\n" for p, t in cases)
    proc = subprocess.run(
        [HARNESS], input=payload, capture_output=True, text=True
    )
    rust_lines = proc.stdout.splitlines()

    if len(rust_lines) != len(cases):
        print(f"WARN: harness returned {len(rust_lines)} lines for {len(cases)} cases")

    divergences = []
    panics = []
    compile_errs = []
    for (pat, text), exp, got in zip(cases, oracle, rust_lines):
        if got == "P":
            panics.append((pat, text))
        elif got == "E":
            compile_errs.append((pat, text))
        else:
            rust = got == "1"
            if rust != exp:
                divergences.append((pat, text, exp, rust))

    print(f"cases compared: {len(cases)}  (seed={seed})")
    print(f"  match divergences: {len(divergences)}")
    print(f"  rust panics:       {len(panics)}")
    print(f"  rust compile-errs (mrab accepted): {len(compile_errs)}")

    def show(title, rows, fmt):
        if not rows:
            return
        print(f"\n=== {title} (showing up to 25) ===")
        for row in rows[:25]:
            print(fmt(row))

    show("PANICS", panics, lambda r: f"  pat={r[0]!r:30} text={r[1]!r}")
    show("MATCH DIVERGENCES", divergences,
         lambda r: f"  pat={r[0]!r:30} text={r[1]!r:24} mrab={r[2]} rust={r[3]}")
    show("COMPILE DIVERGENCES", compile_errs,
         lambda r: f"  pat={r[0]!r:30} text={r[1]!r}")

    # nonzero exit if any panic or divergence, for CI use
    sys.exit(1 if (panics or divergences) else 0)


if __name__ == "__main__":
    main()
