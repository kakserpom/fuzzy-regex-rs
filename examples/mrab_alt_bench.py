#!/usr/bin/env python3
"""Benchmark mrab-regex alternation (multi-pattern) fuzzy matching."""

import time
import regex  # mrab-regex

def bench(name, iterations, func):
    """Run benchmark and return microseconds per iteration."""
    # Warmup
    for _ in range(50):
        func()

    start = time.perf_counter()
    found_count = 0
    for _ in range(iterations):
        if func():
            found_count += 1
    elapsed = time.perf_counter() - start

    per_iter_us = (elapsed * 1_000_000) / iterations
    print(f"{name:50} {per_iter_us:>10.2f} µs/iter  (found: {found_count})")
    return per_iter_us

def main():
    print("Python mrab-regex Alternation Benchmark")
    print("=======================================\n")

    # Test text
    short_text = "The quick brown fox jumps over the lazy dog."
    medium_text = ("Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
        "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. "
        "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.")
    long_text = medium_text * 20
    dna = ''.join(['ACGT'[i % 4] for i in range(1000)])

    # Test 1: Simple alternation (2 patterns)
    print("Test 1: 2 alternatives")
    re1 = regex.compile(r"(?:quick|lazy){e<=1}", flags=regex.BESTMATCH)
    bench("  (quick|lazy) e<=1 in short", 10_000, lambda: bool(re1.search(short_text)))

    # Test 2: 3 alternatives
    print("\nTest 2: 3 alternatives")
    re2 = regex.compile(r"(?:quick|brown|lazy){e<=1}", flags=regex.BESTMATCH)
    bench("  (quick|brown|lazy) e<=1 in short", 10_000, lambda: bool(re2.search(short_text)))

    # Test 3: 5 alternatives
    print("\nTest 3: 5 alternatives")
    re3 = regex.compile(r"(?:quick|brown|fox|jumps|lazy){e<=1}", flags=regex.BESTMATCH)
    bench("  (quick|brown|fox|jumps|lazy) e<=1 short", 10_000, lambda: bool(re3.search(short_text)))

    # Test 4: Alternation in medium text
    print(f"\nTest 4: Alternation in medium text ({len(medium_text)} bytes)")
    re4 = regex.compile(r"(?:Lorem|ipsum|dolor){e<=2}", flags=regex.BESTMATCH)
    bench("  (Lorem|ipsum|dolor) e<=2", 10_000, lambda: bool(re4.search(medium_text)))

    # Test 5: Alternation in long text
    print(f"\nTest 5: Alternation in long text ({len(long_text)} bytes)")
    re5 = regex.compile(r"(?:Lorem|ipsum|dolor){e<=2}", flags=regex.BESTMATCH)
    bench("  (Lorem|ipsum|dolor) e<=2", 1_000, lambda: bool(re5.search(long_text)))

    # Test 6: DNA motifs (3 alternatives)
    print("\nTest 6: DNA motifs (3 alternatives)")
    re6 = regex.compile(r"(?:ACGTACGT|TGCATGCA|GGCCGGCC){e<=2}", flags=regex.BESTMATCH)
    bench("  DNA 3 motifs e<=2", 10_000, lambda: bool(re6.search(dna)))

    # Test 7: No match with alternation
    print("\nTest 7: No match (alternation)")
    re7 = regex.compile(r"(?:zzzzz|xxxxx|yyyyy){e<=1}", flags=regex.BESTMATCH)
    bench("  (zzzzz|xxxxx|yyyyy) no match short", 10_000, lambda: bool(re7.search(short_text)))
    bench("  (zzzzz|xxxxx|yyyyy) no match medium", 1_000, lambda: bool(re7.search(medium_text)))

    # Test 8: Longer alternatives
    print("\nTest 8: Longer alternatives")
    re8 = regex.compile(r"(?:consectetur|adipiscing|exercitation){e<=2}", flags=regex.BESTMATCH)
    bench("  3 long words e<=2 in medium", 10_000, lambda: bool(re8.search(medium_text)))

    # Test 9: 10 alternatives
    print("\nTest 9: 10 alternatives")
    re9 = regex.compile(r"(?:the|quick|brown|fox|jumps|over|lazy|dog|lorem|ipsum){e<=1}", flags=regex.BESTMATCH)
    bench("  10 alternatives e<=1 in short", 10_000, lambda: bool(re9.search(short_text)))

    # Test 10: Compare single vs alternation
    print("\nTest 10: Single pattern vs alternation")
    re_single = regex.compile(r"(?:quick){e<=1}", flags=regex.BESTMATCH)
    re_alt2 = regex.compile(r"(?:quick|xxxxx){e<=1}", flags=regex.BESTMATCH)
    re_alt5 = regex.compile(r"(?:quick|xxxxx|yyyyy|zzzzz|wwwww){e<=1}", flags=regex.BESTMATCH)

    bench("  single: quick e<=1", 10_000, lambda: bool(re_single.search(short_text)))
    bench("  2 alts: (quick|xxxxx) e<=1", 10_000, lambda: bool(re_alt2.search(short_text)))
    bench("  5 alts: (quick|xxxxx|...) e<=1", 10_000, lambda: bool(re_alt5.search(short_text)))

    print("\nDone!")

if __name__ == "__main__":
    main()
