# Compatibility Layer

Migrate from other fuzzy matching libraries.

## fuzzy-aho-corasick

This is a drop-in replacement for `fuzzy-aho-corasick`:

```rust
use fuzzy_regex::compat::fac::FuzzyAhoCorasickBuilder;
use fuzzy_regex::types::FuzzyLimits;

let searcher = FuzzyAhoCorasickBuilder::new()
    .fuzzy(FuzzyLimits::new().edits(1))
    .build(["hello", "world"])
    .unwrap();

for m in searcher.find_iter("helo wrld") {
    println!("Pattern {} matched at {}", m.pattern_index(), m.start());
}
```
