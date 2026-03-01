#!/usr/bin/env python3
"""
Benchmark script for mrab-regex (Python regex module with fuzzy matching).

This script runs the same benchmarks as the Rust fuzzy-regex benchmarks
for comparison purposes.

Requirements:
    pip install regex

Usage:
    python benches/mrab_benchmarks.py
"""

import time
import statistics
import regex  # mrab-regex

# Sample texts
SHORT_TEXT = "The quick brown fox jumps over the lazy dog."
MEDIUM_TEXT = """Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur."""

LONG_TEXT = MEDIUM_TEXT * 100
VERY_LONG_TEXT = MEDIUM_TEXT * 1000


def benchmark(func, iterations=1000, warmup=100):
    """Run a benchmark and return statistics."""
    # Warmup
    for _ in range(warmup):
        func()

    # Actual benchmark
    times = []
    for _ in range(iterations):
        start = time.perf_counter_ns()
        func()
        end = time.perf_counter_ns()
        times.append(end - start)

    return {
        'mean_ns': statistics.mean(times),
        'median_ns': statistics.median(times),
        'stdev_ns': statistics.stdev(times) if len(times) > 1 else 0,
        'min_ns': min(times),
        'max_ns': max(times),
        'iterations': iterations,
    }


def format_time(ns):
    """Format nanoseconds to human-readable string."""
    if ns < 1000:
        return f"{ns:.1f} ns"
    elif ns < 1_000_000:
        return f"{ns/1000:.2f} us"
    elif ns < 1_000_000_000:
        return f"{ns/1_000_000:.2f} ms"
    else:
        return f"{ns/1_000_000_000:.2f} s"


def print_result(name, result):
    """Print benchmark result."""
    print(f"  {name}:")
    print(f"    mean:   {format_time(result['mean_ns'])}")
    print(f"    median: {format_time(result['median_ns'])}")
    print(f"    stdev:  {format_time(result['stdev_ns'])}")


def run_benchmarks():
    print("=" * 60)
    print("mrab-regex Benchmarks")
    print("=" * 60)
    print()

    # Exact match
    print("exact_match_short:")
    re_exact = regex.compile(r"quick")
    result = benchmark(lambda: re_exact.search(SHORT_TEXT))
    print_result("search", result)
    print()

    # Fuzzy match with 1 edit
    print("fuzzy_1_edit_short:")
    re_fuzzy_1 = regex.compile(r"(?:quikc){e<=1}")
    result = benchmark(lambda: re_fuzzy_1.search(SHORT_TEXT))
    print_result("search", result)
    print()

    # Fuzzy match with 2 edits
    print("fuzzy_2_edits_short:")
    re_fuzzy_2 = regex.compile(r"(?:qwick){e<=2}")
    result = benchmark(lambda: re_fuzzy_2.search(SHORT_TEXT))
    print_result("search", result)
    print()

    # Fuzzy match with substitution constraint
    print("fuzzy_substitution_short:")
    re_sub = regex.compile(r"(?:quack){s<=2}")
    result = benchmark(lambda: re_sub.search(SHORT_TEXT))
    print_result("search", result)
    print()

    # Fuzzy match with cost constraint
    print("fuzzy_cost_constraint_short:")
    re_cost = regex.compile(r"(?:quikc){1i+1d<3}")
    result = benchmark(lambda: re_cost.search(SHORT_TEXT))
    print_result("search", result)
    print()

    # Text size scaling
    print("text_size_scaling:")
    re_lorem = regex.compile(r"(?:lorem){e<=2}", regex.IGNORECASE)

    result_medium = benchmark(lambda: re_lorem.search(MEDIUM_TEXT), iterations=500)
    print_result("medium_text", result_medium)

    result_long = benchmark(lambda: re_lorem.search(LONG_TEXT), iterations=100)
    print_result("long_text", result_long)

    result_very_long = benchmark(lambda: re_lorem.search(VERY_LONG_TEXT), iterations=10)
    print_result("very_long_text", result_very_long)
    print()

    # Pattern length scaling
    print("pattern_length_scaling:")
    re_short = regex.compile(r"(?:lorem){e<=1}", regex.IGNORECASE)
    re_medium = regex.compile(r"(?:consectetur){e<=2}", regex.IGNORECASE)
    re_long = regex.compile(r"(?:exercitation){e<=2}", regex.IGNORECASE)

    result = benchmark(lambda: re_short.search(LONG_TEXT), iterations=100)
    print_result("pattern_5_chars", result)

    result = benchmark(lambda: re_medium.search(LONG_TEXT), iterations=100)
    print_result("pattern_11_chars", result)

    result = benchmark(lambda: re_long.search(LONG_TEXT), iterations=100)
    print_result("pattern_13_chars", result)
    print()

    # Edit distance scaling
    print("edit_distance_scaling:")
    re_0 = regex.compile(r"(?:lorem){e<=0}", regex.IGNORECASE)
    re_1 = regex.compile(r"(?:lorem){e<=1}", regex.IGNORECASE)
    re_2 = regex.compile(r"(?:lorem){e<=2}", regex.IGNORECASE)
    re_3 = regex.compile(r"(?:lorem){e<=3}", regex.IGNORECASE)

    result = benchmark(lambda: re_0.search(LONG_TEXT), iterations=100)
    print_result("0_edits", result)

    result = benchmark(lambda: re_1.search(LONG_TEXT), iterations=100)
    print_result("1_edit", result)

    result = benchmark(lambda: re_2.search(LONG_TEXT), iterations=100)
    print_result("2_edits", result)

    result = benchmark(lambda: re_3.search(LONG_TEXT), iterations=100)
    print_result("3_edits", result)
    print()

    # find_iter (findall for multiple matches)
    print("find_iter:")
    re_dolor = regex.compile(r"(?:dolor){e<=1}", regex.IGNORECASE)
    result = benchmark(lambda: list(re_dolor.finditer(LONG_TEXT)), iterations=100)
    print_result("find_iter_long_text", result)
    print()

    # is_match (boolean check)
    print("is_match:")
    re_found = regex.compile(r"(?:lorem){e<=2}", regex.IGNORECASE)
    re_not_found = regex.compile(r"(?:xyzzy){e<=1}")

    result = benchmark(lambda: re_found.search(LONG_TEXT) is not None, iterations=100)
    print_result("is_match_found", result)

    result = benchmark(lambda: re_not_found.search(LONG_TEXT) is not None, iterations=100)
    print_result("is_match_not_found", result)
    print()

    # Compilation
    print("compilation:")
    result = benchmark(lambda: regex.compile(r"(?:hello){e<=2}"), iterations=1000)
    print_result("simple_pattern", result)

    result = benchmark(lambda: regex.compile(r"(?:hello){i<=1,d<=1,s<=2,1i+1d<3}"), iterations=1000)
    print_result("complex_pattern", result)
    print()

    # Typo correction
    print("typo_correction:")
    document = """The recieve function should recieve data from the server. \
Make sure to recieve all packets before processing. \
If you don't recieve a response within 5 seconds, retry."""

    re_typo = regex.compile(r"(?:receive){e<=2}")
    result = benchmark(lambda: list(re_typo.finditer(document)), iterations=1000)
    print_result("find_misspellings", result)
    print()

    # DNA sequence matching
    print("dna_matching:")
    dna = ''.join(['ACGT'[i % 4] for i in range(10000)])
    re_dna = regex.compile(r"(?:ACGTACGT){e<=2}")
    result = benchmark(lambda: re_dna.search(dna), iterations=100)
    print_result("find_motif", result)
    print()

    print("=" * 60)
    print("Benchmarks complete!")
    print("=" * 60)


if __name__ == "__main__":
    run_benchmarks()
