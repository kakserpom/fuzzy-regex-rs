#!/usr/bin/env python3
"""Benchmark mrab-regex for comparison with fuzzy-regex."""

import time
import regex  # mrab-regex

def bench(name, iterations, func):
    """Run benchmark and return microseconds per iteration."""
    # Warmup
    for _ in range(5):
        func()

    start = time.perf_counter()
    for _ in range(iterations):
        func()
    elapsed = time.perf_counter() - start

    per_iter_us = (elapsed * 1_000_000) / iterations
    print(f"{name:50} {per_iter_us:>12.2f} us/iter")
    return per_iter_us

def main():
    print("Python mrab-regex Benchmark")
    print("===========================\n")

    # Test 1: Short text, simple fuzzy
    short_text = "The quick brown fox jumps over the lazy dog."
    print(f"Test 1: Short text ({len(short_text)} bytes)")

    re1 = regex.compile(r"(?:quick){e<=1}")
    bench("  find 'quick' with e<=1", 10000, lambda: re1.search(short_text))

    # Test 2: Medium text
    medium_text = ("Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
        "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. "
        "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.")
    print(f"\nTest 2: Medium text ({len(medium_text)} bytes)")

    re2 = regex.compile(r"(?:Lorem){e<=2}")
    bench("  find 'Lorem' with e<=2", 1000, lambda: re2.search(medium_text))

    # Test 3: Long text (4KB)
    long_text = medium_text * 20
    print(f"\nTest 3: Long text ({len(long_text)} bytes)")

    re3 = regex.compile(r"(?:Lorem){e<=2}")
    bench("  find 'Lorem' with e<=2", 100, lambda: re3.search(long_text))

    # Test 4: Pattern matching with substitution constraint
    print("\nTest 4: Substitution constraint")
    re4 = regex.compile(r"(?:quick){s<=1}")
    bench("  find 'quick' with s<=1 (short)", 10000, lambda: re4.search(short_text))

    # Test 5: No match (worst case - full scan)
    print("\nTest 5: No match (full scan)")
    re5 = regex.compile(r"(?:xyzzy){e<=1}")
    bench("  find 'xyzzy' e<=1 (short, no match)", 10000, lambda: re5.search(short_text))
    bench("  find 'xyzzy' e<=1 (medium, no match)", 1000, lambda: re5.search(medium_text))

    # Test 6: DNA sequence
    print("\nTest 6: DNA sequence (1000 bp)")
    dna = ''.join(['ACGT'[i % 4] for i in range(1000)])
    re6 = regex.compile(r"(?:ACGTACGT){e<=2}")
    bench("  find motif with e<=2", 100, lambda: re6.search(dna))

    # Test 7: BESTMATCH mode (most similar to greedy_first)
    print("\n=== BESTMATCH MODE ===\n")

    print("Test 7: Short text with BESTMATCH")
    re7 = regex.compile(r"(?:quick){e<=1}", flags=regex.BESTMATCH)
    bench("  find 'quick' with e<=1 (bestmatch)", 10000, lambda: re7.search(short_text))

    print("\nTest 8: Medium text with BESTMATCH")
    re8 = regex.compile(r"(?:Lorem){e<=2}", flags=regex.BESTMATCH)
    bench("  find 'Lorem' with e<=2 (bestmatch)", 1000, lambda: re8.search(medium_text))

    print("\nTest 9: Long text with BESTMATCH")
    re9 = regex.compile(r"(?:Lorem){e<=2}", flags=regex.BESTMATCH)
    bench("  find 'Lorem' with e<=2 (bestmatch)", 100, lambda: re9.search(long_text))

    print("\nTest 10: DNA with BESTMATCH")
    re10 = regex.compile(r"(?:ACGTACGT){e<=2}", flags=regex.BESTMATCH)
    bench("  find motif with e<=2 (bestmatch)", 100, lambda: re10.search(dna))

    print("\nDone!")

if __name__ == "__main__":
    main()
