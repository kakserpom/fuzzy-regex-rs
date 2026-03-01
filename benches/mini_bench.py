import time
import regex

text = "The quick brown fox jumps over the lazy dog."
long_text = text * 100

print("mrab-regex benchmarks (us/iter)")
print("================================")

# Short text
re1 = regex.compile(r"(?:quikc){e<=1}")
start = time.perf_counter()
for _ in range(1000): re1.search(text)
print(f"short text, 1 edit:    {(time.perf_counter()-start)*1000:>8.1f}")

re2 = regex.compile(r"(?:qwick){e<=2}")
start = time.perf_counter()
for _ in range(1000): re2.search(text)
print(f"short text, 2 edits:   {(time.perf_counter()-start)*1000:>8.1f}")

# Long text
re3 = regex.compile(r"(?:lorem){e<=2}", regex.IGNORECASE)
start = time.perf_counter()
for _ in range(10): re3.search(long_text)
print(f"long text, 2 edits:    {(time.perf_counter()-start)*100000:>8.1f}")

re4 = regex.compile(r"(?:xyzzy){e<=1}")
start = time.perf_counter()
for _ in range(10): re4.search(long_text)
print(f"long text, no match:   {(time.perf_counter()-start)*100000:>8.1f}")
