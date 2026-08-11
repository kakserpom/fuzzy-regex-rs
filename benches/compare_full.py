#!/usr/bin/env python3
"""Comprehensive benchmark for mrab-regex to compare with fuzzy-regex.

Run fuzzy-regex first: cargo run --release --example compare_full
Then run this: python3 examples/compare_full.py
"""

import time
import regex  # mrab-regex

def bench(name, iterations, func):
    """Run benchmark and return microseconds per iteration."""
    # Warmup
    for _ in range(100):
        func()

    start = time.perf_counter()
    found = 0
    for _ in range(iterations):
        if func():
            found += 1
    elapsed = time.perf_counter() - start

    per_iter_us = (elapsed * 1_000_000) / iterations
    print(f"{name:55} {per_iter_us:>8.2f} µs  (found: {found}/{iterations})")
    return per_iter_us

def main():
    print("=== mrab-regex Comprehensive Benchmark ===\n")

    # ============================================
    # SECTION 1: Text Length Scaling
    # ============================================
    print("--- 1. Text Length Scaling (pattern at start) ---")
    base_text = "The quick brown fox jumps over the lazy dog. "
    for length in [50, 100, 500, 1000, 5000, 10000]:
        text = (base_text * ((length // len(base_text)) + 1))[:length]
        re_obj = regex.compile(r"(?:quick){e<=1}", flags=regex.BESTMATCH)
        bench(f"'quick' e<=1 in {length} chars", 10_000, lambda: bool(re_obj.search(text)))

    # ============================================
    # SECTION 2: Match Position Impact
    # ============================================
    print("\n--- 2. Match Position Impact (1000 char text) ---")
    text_1000 = "X" * 1000
    for pos in [0, 10, 50, 100, 500, 900]:
        text = text_1000[:pos] + "quick" + text_1000[pos+5:]
        re_obj = regex.compile(r"(?:quick){e<=1}", flags=regex.BESTMATCH)
        bench(f"'quick' at position {pos}", 10_000, lambda: bool(re_obj.search(text)))

    # ============================================
    # SECTION 3: Edit Distance Scaling
    # ============================================
    print("\n--- 3. Edit Distance Scaling ---")
    text = "The quikc brown fox jumps over the lazy dog."
    for e in range(5):
        re_obj = regex.compile(rf"(?:quick){{e<={e}}}", flags=regex.BESTMATCH)
        bench(f"'quick' e<={e}", 10_000, lambda: bool(re_obj.search(text)))

    # ============================================
    # SECTION 4: Pattern Length Scaling
    # ============================================
    print("\n--- 4. Pattern Length Scaling (e<=2) ---")
    long_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. " * 20
    patterns = [
        ("Lorem", 5),
        ("consectetur", 11),
        ("adipiscing elit", 15),
        ("Lorem ipsum dolor", 17),
        ("consectetur adipiscing", 22),
    ]
    for pat, length in patterns:
        re_obj = regex.compile(rf"(?:{regex.escape(pat)}){{e<=2}}", flags=regex.BESTMATCH)
        bench(f"'{pat[:15]}' ({length} chars) e<=2", 10_000, lambda: bool(re_obj.search(long_text)))

    # ============================================
    # SECTION 5: DNA/Bioinformatics
    # ============================================
    print("\n--- 5. DNA/Bioinformatics ---")
    dna = ''.join(['ACGT'[i % 4] for i in range(10000)])

    # Different motif lengths
    for motif_len in [4, 8, 12, 16, 20]:
        motif = ''.join(['ACGT'[i % 4] for i in range(motif_len)])
        re_obj = regex.compile(rf"(?:{motif}){{e<=2}}", flags=regex.BESTMATCH)
        bench(f"DNA motif {motif_len} bp, e<=2, 10kb", 1_000, lambda: bool(re_obj.search(dna)))

    # Different DNA sizes
    print("\n--- 5b. DNA Size Scaling ---")
    motif = "ACGTACGT"
    for size in [100, 1000, 10000, 100000]:
        dna = ''.join(['ACGT'[i % 4] for i in range(size)])
        re_obj = regex.compile(rf"(?:{motif}){{e<=2}}", flags=regex.BESTMATCH)
        iters = 100 if size > 10000 else 1_000
        bench(f"ACGTACGT e<=2 in {size} bp", iters, lambda: bool(re_obj.search(dna)))

    # ============================================
    # SECTION 6: No Match (Worst Case)
    # ============================================
    print("\n--- 6. No Match (Full Scan) ---")
    for size in [100, 500, 1000, 5000]:
        text = "X" * size
        re_obj = regex.compile(r"(?:quick){e<=1}", flags=regex.BESTMATCH)
        iters = 1_000 if size > 1000 else 10_000
        bench(f"No match in {size} chars", iters, lambda: bool(re_obj.search(text)))

    # ============================================
    # SECTION 7: Alternation Patterns
    # ============================================
    print("\n--- 7. Alternation Patterns ---")
    text = "The quick brown fox jumps over the lazy dog."

    alt_patterns = [
        (r"(?:quick|slow){e<=1}", "2 alts, short"),
        (r"(?:quick|brown|lazy){e<=1}", "3 alts, short"),
        (r"(?:the|quick|brown|fox|jumps){e<=1}", "5 alts, short"),
        (r"(?:quick|brown|fox|jumps|over|lazy|dog|the|a|an){e<=1}", "10 alts"),
    ]
    for pattern, desc in alt_patterns:
        re_obj = regex.compile(pattern, flags=regex.BESTMATCH)
        bench(f"{desc}", 10_000, lambda: bool(re_obj.search(text)))

    # ============================================
    # SECTION 8: Case Insensitive
    # ============================================
    print("\n--- 8. Case Insensitive ---")
    text = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG."
    re_ci = regex.compile(r"(?:quick){e<=1}", flags=regex.BESTMATCH | regex.IGNORECASE)
    re_cs = regex.compile(r"(?:QUICK){e<=1}", flags=regex.BESTMATCH)
    bench("Case insensitive 'quick' e<=1", 10_000, lambda: bool(re_ci.search(text)))
    bench("Case sensitive 'QUICK' e<=1", 10_000, lambda: bool(re_cs.search(text)))

    # ============================================
    # SECTION 9: Real-World Patterns
    # ============================================
    print("\n--- 9. Real-World Patterns ---")

    # Email-like pattern
    text_email = "Contact us at support@example.com for more information."
    re_email = regex.compile(r"(?:support){e<=2}", flags=regex.BESTMATCH)
    bench("Email prefix 'support' e<=2", 10_000, lambda: bool(re_email.search(text_email)))

    # Name matching
    names = "John Smith, Jane Doe, Robert Johnson, Michael Williams, David Brown"
    re_name = regex.compile(r"(?:Johnson){e<=2}", flags=regex.BESTMATCH)
    bench("Name 'Johnson' e<=2", 10_000, lambda: bool(re_name.search(names)))

    # Address matching
    address = "123 Main Street, Springfield, IL 62701"
    re_addr = regex.compile(r"(?:Springfield){e<=2}", flags=regex.BESTMATCH)
    bench("City 'Springfield' e<=2", 10_000, lambda: bool(re_addr.search(address)))

    # Product code
    products = "SKU: ABC-12345-XYZ, Price: $99.99, Stock: 150 units"
    re_sku = regex.compile(r"(?:ABC-12345){e<=1}", flags=regex.BESTMATCH)
    bench("SKU 'ABC-12345' e<=1", 10_000, lambda: bool(re_sku.search(products)))

    # ============================================
    # SECTION 10: Unicode/International
    # ============================================
    print("\n--- 10. Unicode/International ---")
    text_unicode = "Привет мир! Hello world! 你好世界！"
    re_ru = regex.compile(r"(?:Привет){e<=1}", flags=regex.BESTMATCH)
    re_zh = regex.compile(r"(?:你好){e<=1}", flags=regex.BESTMATCH)
    bench("Russian 'Привет' e<=1", 10_000, lambda: bool(re_ru.search(text_unicode)))
    bench("Chinese '你好' e<=1", 10_000, lambda: bool(re_zh.search(text_unicode)))

    # ============================================
    # SECTION 11: Exact vs Fuzzy
    # ============================================
    print("\n--- 11. Exact vs Fuzzy Match ---")
    text = "The quick brown fox jumps over the lazy dog."
    for e in [0, 1, 2, 3]:
        re_obj = regex.compile(rf"(?:quick){{e<={e}}}", flags=regex.BESTMATCH)
        bench(f"Exact text, e<={e}", 10_000, lambda: bool(re_obj.search(text)))

    # ============================================
    # SECTION 12: Multiple Matches (findall)
    # ============================================
    print("\n--- 12. Multiple Matches ---")
    text_repeat = "cat bat rat cat bat rat cat bat rat " * 10
    re_multi = regex.compile(r"(?:cat|bat|rat){e<=1}", flags=regex.BESTMATCH)
    bench("Count all matches (cat|bat|rat)", 1_000, lambda: len(re_multi.findall(text_repeat)) > 0)

    print("\n=== Benchmark Complete ===")

if __name__ == "__main__":
    main()
