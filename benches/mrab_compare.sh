#!/usr/bin/env bash
# Run both sides of the fuzzy-regex vs mrab comparison and print a table.
# Requires: cargo (release build) and `python3 -m pip install regex`.
# Run from anywhere: bash benches/mrab_compare.sh
set -eu
cd "$(dirname "$0")/.."

echo "building rust harness (release)..." >&2
cargo build --release --example mrab_compare >/dev/null 2>&1

r=$(mktemp)
m=$(mktemp)
trap 'rm -f "$r" "$m"' EXIT
cargo run --release --example mrab_compare 2>/dev/null >"$r"
python3 benches/mrab_compare.py 2>/dev/null >"$m"
cat "$r" "$m" | python3 benches/mrab_compare_merge.py
