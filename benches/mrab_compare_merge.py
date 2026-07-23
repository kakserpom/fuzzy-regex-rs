#!/usr/bin/env python3
"""Merge the RUST and MRAB benchmark lines (on stdin) into a comparison table.

Reads `RUST<TAB>name<TAB>ns<TAB>result` and `MRAB<TAB>name<TAB>ns<TAB>result`
lines and prints a side-by-side table with a speed ratio (>1 = fuzzy-regex
faster) and a match-agreement flag.
"""

import sys


def canon(s):
    """Canonicalise a result string so cosmetic formatting (Rust `Some((a, b))`
    vs Python `(a, b)`, `true` vs `True`) doesn't read as a mismatch."""
    return (
        s.lower()
        .replace("some", "")
        .replace("(", "")
        .replace(")", "")
        .replace(" ", "")
        .strip()
    )


def main():
    ours, mrab, order = {}, {}, []
    for line in sys.stdin:
        p = line.rstrip("\n").split("\t")
        if len(p) < 3:
            continue
        who, name, ns = p[0], p[1], p[2]
        res = p[3] if len(p) > 3 else ""
        if who == "RUST":
            ours[name] = (ns, res)
            order.append(name)
        elif who == "MRAB":
            mrab[name] = (ns, res)

    hdr = f"{'case':24} {'ours(ns)':>11} {'mrab(ns)':>11} {'ratio':>14}  match"
    print(hdr)
    print("-" * len(hdr))
    faster = slower = 0
    for name in order:
        ons, ores = ours.get(name, ("?", ""))
        mns, mres = mrab.get(name, ("?", ""))
        if ons == "ERR" or mns == "ERR":
            who = "mrab rejects" if mns == "ERR" else "ours rejects"
            print(f"{name:24} {ons:>11} {mns:>11} {'-':>14}  {who}")
            continue
        o, m = float(ons), float(mns)
        r = m / o
        if r >= 1:
            tag = f"{r:>6.1f}x faster"
            faster += 1
        else:
            tag = f"{o / m:>6.1f}x slower"
            slower += 1
        match = "same" if canon(ores) == canon(mres) else f"DIFF ours={ores} mrab={mres}"
        print(f"{name:24} {o:>11.1f} {m:>11.1f} {tag:>14}  {match}")
    print("-" * len(hdr))
    print(f"fuzzy-regex faster on {faster} cases, slower on {slower} "
          f"(lower ns = faster; Python-loop overhead inflates mrab's sub-µs cases)")


if __name__ == "__main__":
    main()
