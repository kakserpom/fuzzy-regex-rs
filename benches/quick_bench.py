#!/usr/bin/env python3
"""
Quick benchmark for mrab-regex comparison.
Run with: python benches/quick_bench.py

Requires: pip install regex
"""

import time
import regex

SHORT_TEXT = "The quick brown fox jumps over the lazy dog."
MEDIUM_TEXT = """Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur."""

LONG_TEXT = MEDIUM_TEXT * 100


def bench(name, iterations, func):
    """Run benchmark and print result."""
    # Warmup
    for _ in range(10):
        func()

    start = time.perf_counter()
    for _ in range(iterations):
        func()
    elapsed = time.perf_counter() - start

    per_iter_us = (elapsed / iterations) * 1_000_000
    print(f"{name:40} {per_iter_us:>10.2f} us/iter ({iterations} iters)")


def main():
    print("mrab-regex Quick Benchmarks")
    print("===========================")
    print()

    # Compilation benchmarks
    print("--- Compilation ---")
    bench("compile simple pattern", 1000, lambda: regex.compile(r"(?:hello){e<=2}"))
    bench("compile complex pattern", 1000, lambda: regex.compile(r"(?:hello){i<=1,d<=1,s<=2,1i+1d<3}"))
    print()

    # Short text benchmarks
    print(f"--- Short text ({len(SHORT_TEXT)} bytes) ---")
    re_exact = regex.compile(r"quick")
    bench("exact match", 10000, lambda: re_exact.search(SHORT_TEXT))

    re_fuzzy_1 = regex.compile(r"(?:quikc){e<=1}")
    bench("fuzzy 1 edit", 10000, lambda: re_fuzzy_1.search(SHORT_TEXT))

    re_fuzzy_2 = regex.compile(r"(?:qwick){e<=2}")
    bench("fuzzy 2 edits", 10000, lambda: re_fuzzy_2.search(SHORT_TEXT))

    re_sub = regex.compile(r"(?:quack){s<=2}")
    bench("substitution constraint", 10000, lambda: re_sub.search(SHORT_TEXT))

    re_cost = regex.compile(r"(?:quikc){1i+1d<3}")
    bench("cost constraint", 10000, lambda: re_cost.search(SHORT_TEXT))
    print()

    # Long text benchmarks
    print(f"--- Long text ({len(LONG_TEXT)} bytes) ---")
    re_lorem = regex.compile(r"(?:lorem){e<=2}", regex.IGNORECASE)
    bench("fuzzy 2 edits (find first)", 100, lambda: re_lorem.search(LONG_TEXT))

    bench("fuzzy 2 edits (findall count)", 100, lambda: len(re_lorem.findall(LONG_TEXT)))

    re_dolor = regex.compile(r"(?:dolor){e<=1}", regex.IGNORECASE)
    bench("fuzzy 1 edit (findall count)", 100, lambda: len(re_dolor.findall(LONG_TEXT)))

    re_no_match = regex.compile(r"(?:xyzzy){e<=1}")
    bench("no match (full scan)", 100, lambda: re_no_match.search(LONG_TEXT))
    print()

    # Edit distance scaling
    print("--- Edit distance scaling (long text) ---")
    re_0 = regex.compile(r"(?:lorem){e<=0}", regex.IGNORECASE)
    re_1 = regex.compile(r"(?:lorem){e<=1}", regex.IGNORECASE)
    re_2 = regex.compile(r"(?:lorem){e<=2}", regex.IGNORECASE)
    re_3 = regex.compile(r"(?:lorem){e<=3}", regex.IGNORECASE)

    bench("0 edits (exact)", 100, lambda: re_0.search(LONG_TEXT))
    bench("1 edit", 100, lambda: re_1.search(LONG_TEXT))
    bench("2 edits", 100, lambda: re_2.search(LONG_TEXT))
    bench("3 edits", 100, lambda: re_3.search(LONG_TEXT))
    print()

    # DNA sequence benchmark
    print("--- DNA sequence (10000 bp) ---")
    dna = ''.join(['ACGT'[i % 4] for i in range(10000)])
    re_dna = regex.compile(r"(?:ACGTACGT){e<=2}")
    bench("find motif", 100, lambda: re_dna.search(dna))
    print()

    print("Done!")


if __name__ == "__main__":
    main()
