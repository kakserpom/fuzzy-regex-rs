# Compatibility Layer

Migrate from other fuzzy matching libraries.

## fuzzy-aho-corasick

This is a drop-in replacement for `fuzzy-aho-corasick`:

```rust
use fuzzy_regex::compat::fac::FuzzyAhoCorasickBuilder;
use fuzzy_regex::types::FuzzyLimits;

let searcher = FuzzyAhoCorasickBuilder::new()
    .fuzzy(FuzzyLimits::new().edits(1))
    .build(["hello", "world"]);

let matches = searcher.search("helo wrld", 0.5);
for m in &matches {
    println!("Pattern {} matched at {}", m.pattern_index, m.start);
}
```
