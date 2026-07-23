#!/usr/bin/env python3
"""mrab-regex side of the fuzzy-regex vs mrab comparison benchmark.

Reads the shared case list `benches/compare_cases.tsv`, times each case with the
Python `regex` (mrab) module, and prints `MRAB<TAB>name<TAB>ns_per_iter<TAB>result`
lines. The corpora below must match `examples/mrab_compare.rs` exactly.

Run with: python3 benches/mrab_compare.py
"""

import os
import time

import regex  # mrab-regex


def corpora():
    short = "The quick brown fox jumps over the lazy dog."
    medium = (
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
        "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. "
        "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris."
    )
    long = medium * 20
    dna = "ACGT" * 250
    repeats = "alpha beta gamma delta beta epsilon zeta beta"
    code = "x(a(b)c)(d(e(f)g)h)y " * 50
    unicode = "grüße die straße weiß im FUSSBALL"
    return {
        "short": short,
        "medium": medium,
        "long": long,
        "dna": dna,
        "repeats": repeats,
        "code": code,
        "unicode": unicode,
    }


def run_op(rx, op, text):
    if op == "find":
        return rx.search(text)
    if op == "find_iter":
        return sum(1 for _ in rx.finditer(text))
    if op == "is_match":
        return rx.search(text) is not None
    raise ValueError(f"unknown op {op}")


def result_str(op, rx, text):
    if op == "find":
        m = rx.search(text)
        return str(m.span()) if m else "None"
    if op == "find_iter":
        return f"n={sum(1 for _ in rx.finditer(text))}"
    if op == "is_match":
        return str(rx.search(text) is not None)
    return "?"


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    tsv = os.path.join(here, "compare_cases.tsv")
    texts = corpora()

    with open(tsv, encoding="utf-8") as f:
        lines = f.read().splitlines()

    for line in lines[1:]:
        if not line.strip():
            continue
        name, op, corpus, iters, pattern = line.split("\t", 4)
        iters = int(iters)
        text = texts[corpus]

        try:
            rx = regex.compile(pattern)
        except Exception as e:  # noqa: BLE001
            print(f"MRAB\t{name}\tERR\tcompile: {e}")
            continue

        res = result_str(op, rx, text)
        for _ in range(3):
            run_op(rx, op, text)

        start = time.perf_counter_ns()
        for _ in range(iters):
            run_op(rx, op, text)
        ns = (time.perf_counter_ns() - start) / iters
        print(f"MRAB\t{name}\t{ns:.1f}\t{res}")


if __name__ == "__main__":
    main()
