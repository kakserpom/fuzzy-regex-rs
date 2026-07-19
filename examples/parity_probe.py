#!/usr/bin/env python3
"""Feature-parity probe: curated fuzzy-regex vs mrab-regex feature battery.

Checks, for each mrab fuzzy feature, that fuzzy-regex (a) parses the same syntax
and (b) agrees on match existence, span, and fuzzy_counts. Complements the
random differential fuzzer (examples/diff_fuzz.py) with targeted coverage of
every fuzzy syntax construct.

    cargo build --release --example parity_harness
    python3 examples/parity_probe.py
"""
import os
import subprocess
import sys

try:
    import regex
except ImportError:
    sys.exit("pip install regex (mrab-regex) required")

HARNESS = os.path.join(
    os.path.dirname(__file__), "..", "target", "release", "examples", "parity_harness"
)

# (category, pattern, text). Each exercises a distinct mrab fuzzy feature.
CASES = [
    ("total e<=n",        r"(?:hello){e<=2}", "helxo"),
    ("total e<=n miss",   r"(?:hello){e<=1}", "hxxlo"),
    ("insertion i<=n",    r"(?:hello){i<=1}", "hellxo"),
    ("deletion d<=n",     r"(?:hello){d<=1}", "helo"),
    ("substitution s<=n", r"(?:hello){s<=1}", "hallo"),
    ("i only, no d/s",    r"(?:hello){i<=2}", "hallo"),   # sub not allowed -> no match
    ("combined idst",     r"(?:hello){i<=1,d<=1,s<=1}", "hetlo"),
    ("combined + e",      r"(?:foobar){i<=2,s<=2,e<=2}", "foobyr"),
    ("cost eqn",          r"(?:foobar){2i+1d+1s<=4}", "foybar"),
    ("cost eqn 2",        r"(?:foobar){i<=1,d<=2,s<=3,2d+1s<4}", "fobar"),
    ("simple cost c<=n",  r"(?:hello){c<=2}", "helo"),
    ("range 1<=e<=3",     r"(?:hello){1<=e<=3}", "hello"),   # min 1 error required
    ("excl e<n",          r"(?:hello){e<3}", "haxlo"),
    ("excl 0<e<n",        r"(?:hello){0<e<3}", "hello"),     # min 1 error
    ("unlimited e",       r"(?:hello){e}", "hxxxxxxo"),
    ("unlimited i",       r"(?:hel){i}", "hxxel"),
    ("char restr s",      r"(?:hello){s<=1:[a-z]}", "hallo"),
    ("char restr s rej",  r"(?:hello){s<=1:[a-z]}", "h3llo"),   # 3 not in [a-z]
    ("char restr e",      r"(?:hello){e<=1:[a-z]}", "hallo"),
    ("fuzzy on class",    r"(?:[hy]ello){e<=1}", "yello"),
    ("fuzzy on alt",      r"(?:cat|dog){e<=1}", "cot"),
    ("word boundary",     r"\b(?:hello){e<=1}\b", "hallo world"),
    ("anchored",          r"^(?:hello){e<=1}$", "hallo"),
    ("counts: sub",       r"(?:hello){e<=1}", "hallo"),
    ("counts: ins",       r"(?:hello){e<=1}", "helllo"),
    ("counts: del",       r"(?:hello){e<=1}", "helo"),
]


def pct(s: str) -> str:
    return "".join(
        "%%%02X" % ord(c) if c in "\t\n\r%" else c for c in s
    )


def mrab(p, t):
    try:
        re = regex.compile(p)
    except Exception:
        return ("E",)
    try:
        m = re.search(t)
    except Exception:
        return ("E",)
    if m is None:
        return ("0",)
    s, i, d = m.fuzzy_counts  # mrab order: subs, ins, dels
    return ("1", (s, i, d), m.start(), m.end())


def main():
    if not os.path.exists(HARNESS):
        sys.exit(f"build first: cargo build --release --example parity_harness")

    payload = "".join(f"{pct(p)}\t{pct(t)}\n" for _, p, t in CASES)
    proc = subprocess.run([HARNESS], input=payload, capture_output=True, text=True)
    lines = proc.stdout.splitlines()

    ok = 0
    diffs = []
    for (cat, p, t), line in zip(CASES, lines):
        ref = mrab(p, t)
        # parse fuzzy-regex line
        if line in ("E", "P", "0"):
            got = (line,)
        else:
            parts = line.split("\t")
            counts = tuple(int(x) for x in parts[1].split(","))
            got = ("1", counts, int(parts[2]), int(parts[3]))

        # Compare: existence always; counts+span only when both match.
        if ref[0] != got[0]:
            diffs.append((cat, p, t, ref, got, "existence/parse"))
        elif ref[0] == "1":
            # compare fuzzy_counts and span
            if ref[1] != got[1]:
                diffs.append((cat, p, t, ref, got, "fuzzy_counts"))
            elif (ref[2], ref[3]) != (got[2], got[3]):
                diffs.append((cat, p, t, ref, got, "span"))
            else:
                ok += 1
        else:
            ok += 1

    print(f"parity cases: {len(CASES)}   agree: {ok}   diffs: {len(diffs)}\n")
    for cat, p, t, ref, got, why in diffs:
        print(f"  [{why}] {cat}: {p!r} on {t!r}")
        print(f"       mrab={ref}  fuzzy-regex={got}")
    sys.exit(1 if diffs else 0)


if __name__ == "__main__":
    main()
