[![CI](https://github.com/kakserpom/fuzzy-regex-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/kakserpom/fuzzy-regex-rs/actions)
[![crates.io](https://img.shields.io/crates/v/fuzzy-regex.svg)](https://crates.io/crates/fuzzy-regex)
[![docs](https://img.shields.io/badge/docs-book-blue.svg)](https://kakserpom.github.io/fuzzy-regex-rs/)
[![API Docs](https://img.shields.io/badge/docs-rs-blue.svg)](https://docs.rs/fuzzy-regex/)
[![MSRV](https://img.shields.io/badge/MSRV-1.88+-green.svg)](https://blog.rust-lang.org/2025/05/23/Rust-1.88.html)

# fuzzy-regex

A high-performance fuzzy regular expression engine written in Rust that combines traditional regex constructs with
approximate
string matching using Damerau-Levenshtein automata and the Bitap algorithm.

## Features

- **Fuzzy Matching**: Match strings with configurable edit distance tolerance (insertions, deletions, substitutions,
  transpositions)
- **Full Regex Support**: Character classes, quantifiers, groups, alternation, anchors, lookahead
- **Per-Segment Fuzziness**: Control fuzziness for individual parts of a pattern
- **Capture Groups**: Named and numbered capture groups with fuzzy matching
- **Similarity Scoring**: Get match quality scores (0.0 - 1.0)
- **Streaming API**: Process large files and network streams incrementally
- **High Performance**: Bitap algorithm for patterns ≤64 chars, SIMD optimizations
- **Unicode Support**: Full Unicode support including case-insensitive matching

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
fuzzy-regex = "0.1"
```

### Feature Flags

- `simd` (default): Enable SIMD optimizations for faster matching
- `mimalloc`: Use mimalloc allocator for better performance

## Documentation

- [Book](https://kakserpom.github.io/fuzzy-regex-rs/) - Full documentation and guide
- [API Docs](https://docs.rs/fuzzy-regex/) - Rust API reference

### MSRV

The minimum supported Rust version is **1.88**.

## Quick Start

```rust,ignore
use fuzzy_regex::FuzzyRegex;

// Simple fuzzy matching - allows up to 2 edits
let re = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

assert!(re.is_match("hello"));   // Exact match
assert!(re.is_match("helo"));    // 1 deletion
assert!(re.is_match("helllo"));  // 1 insertion
assert!(re.is_match("hallo"));   // 1 substitution
assert!(re.is_match("hlelo"));   // 1 transposition
```

### Unicode Support

By default, `\w`, `\d`, `\s` match ASCII characters only. Enable Unicode mode with `(?u)`:

```rust,ignore
use fuzzy_regex::FuzzyRegexBuilder;

// Without Unicode (ASCII only)
let re1 = FuzzyRegex::new(r"\w+").unwrap();
re1.is_match("hello"); // true
re1.is_match("привет"); // false (Cyrillic not matched)

// With Unicode mode
let re2 = FuzzyRegex::new("(?u)\\w+").unwrap();
re2.is_match("hello"); // true
re2.is_match("привет"); // true (Cyrillic matched)

// Or via builder
let re3 = FuzzyRegexBuilder::new(r"\w+")
    .unicode(true)
    .build()
    .unwrap();
```

## Pattern Syntax

### Fuzziness Markers

| Syntax                     | Description                       |
|----------------------------|-----------------------------------|
| `(?:text){e<=2}`           | Allow up to 2 total edits         |
| `(?:text){i<=1}`           | Allow up to 1 insertion           |
| `(?:text){d<=1}`           | Allow up to 1 deletion            |
| `(?:text){s<=1}`           | Allow up to 1 substitution        |
| `(?:text){t<=1}`           | Allow up to 1 transposition       |
| `(?:text){i<=1,d<=1,s<=1}` | Separate limits per edit type     |
| `(?:text){e<=1:[a-z]}`     | Restrict edits to character class |
| `(?:text){c<=3}`           | Cost-based: total cost ≤ 3        |
| `(?:text){2i+1d+1s+1t<=4}` | Weighted cost constraint          |
| `text~2`                   | Shorthand: allow up to 2 edits    |

### Standard Regex Constructs

| Syntax                 | Description                                                 |
|------------------------|-------------------------------------------------------------|
| `[a-z]`, `[^0-9]`      | Character classes                                           |
| `\d`, `\w`, `\s`       | Predefined classes (digit, word, whitespace)                |
|                        | Note: By default matches ASCII only. Use `(?u)` for Unicode |
| `\D`, `\W`, `\S`       | Negated predefined classes                                  |
| `.`                    | Any character (except newline by default)                   |
| `*`, `+`, `?`          | Quantifiers (0+, 1+, 0-1)                                   |
| `*?`, `+?`, `??`       | Lazy quantifiers                                            |
| `{n}`, `{n,}`, `{n,m}` | Repetition counts                                           |
| `(group)`              | Capture group                                               |
| `(?:group)`            | Non-capturing group                                         |
| `(?<name>...)`         | Named capture group                                         |
| `a\|b`                 | Alternation                                                 |
| `^`, `$`               | Start/end anchors                                           |
| `\b`, `\B`             | Word boundary / non-boundary                                |
| `(?=...)`, `(?!...)`   | Positive/negative lookahead                                 |
| `(?<=...)`, `(?<!...)` | Positive/negative lookbehind (fixed-length)                 |
| `\1`, `\2`             | Backreferences                                              |

### Inline Flags

| Flag   | Description                                      |
|--------|--------------------------------------------------|
| `(?i)` | Case insensitive                                 |
| `(?m)` | Multi-line (^ and $ match line boundaries)       |
| `(?s)` | Dot-all (. matches newlines)                     |
| `(?x)` | Verbose mode (ignore whitespace, allow comments) |
| `(?U)` | Ungreedy (swap greedy/lazy behavior)             |
| `(?u)` | Unicode mode (\w, \d, \s match Unicode chars)    |
| `(?b)` | BESTMATCH - find best match within alternatives  |
| `(?e)` | ENHANCEMATCH - enhance match with more edits     |
| `(?p)` | POSIX - find longest match at leftmost position  |

### Advanced Features

| Syntax           | Description                                |
|------------------|--------------------------------------------|
| `\K`             | Reset match start position                 |
| `(?>...)`        | Atomic group (no backtracking)             |
| `*+`, `++`, `?+` | Possessive quantifiers                     |
| `(?R)`, `(?1)`   | Recursive patterns                         |
| `(?<name>...)`   | Named capture group with fuzzy modifier    |
| `\L<name>`       | Named list reference (via `set_word_list`) |
| `\G`             | Match at end of previous match position    |

### API Methods

- `find()` - Find first match
- `find_iter()` - Find all matches iteratively
- `find_rev()` - Find rightmost match
- `find_iter_rev()` - Find all matches in reverse order
- `captures()` - Get capture groups with positions
- `fullmatch()` - Match entire string from start to end
- `is_full_match()` - Check if entire string matches
- `split()` - Split by matches
- `replace()` - Replace matches with replacement string
- `find_with_timeout()` - Match with timeout support

### Partial Matches

Match incomplete/streaming input with partial matching:

```rust,ignore
use fuzzy_regex::FuzzyRegexBuilder;

let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
    .partial(true)
    .build()
    .unwrap();

// Match at end of text - marked as partial
let m = re.find("hello").unwrap();
assert!(m.partial());  // Match reached end of input
```

## Examples

### Basic Fuzzy Search

```rust,ignore
use fuzzy_regex::FuzzyRegexBuilder;

let re = FuzzyRegexBuilder::new("(?:teh){e<=1}")
.similarity(0.7)
.build()
.unwrap();

// Finds "the" even when misspelled as "teh"
let m = re.find("I saw teh cat").unwrap();
assert_eq!(m.as_str(), "teh");
```

### Fuzzy Search with Context

```rust
use fuzzy_regex::FuzzyRegex;

// Mix exact and fuzzy matching
let re = FuzzyRegex::new(r"The (?:quick){e<=1} brown").unwrap();

assert!(re.is_match("The quick brown fox"));  // Exact
assert!(re.is_match("The quikc brown fox"));  // Typo in "quick"
```

### Capture Groups

```rust,ignore
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new(r"(?<user>\w+)@(?<domain>\w+\.\w+)").unwrap();
let caps = re.captures("john@example.com").unwrap();

assert_eq!(caps.name("user").unwrap().as_str(), "john");
assert_eq!(caps.name("domain").unwrap().as_str(), "example.com");
```

### Find All Matches

```rust,ignore
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new(r"\b\w+\b").unwrap();
let matches: Vec<_ > = re.find_iter("hello world").collect();

assert_eq!(matches.len(), 2);
assert_eq!(matches[0].as_str(), "hello");
assert_eq!(matches[1].as_str(), "world");
```

### Replace

```rust,ignore
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new("world").unwrap();

// Replace first occurrence
let result = re.replace("hello world world", "rust");
assert_eq!(result, "hello rust world");

// Replace all occurrences
let result = re.replace_all("hello world world", "rust");
assert_eq!(result, "hello rust rust");
```

### Streaming API

Process large files or network streams without loading everything into memory:

```rust,ignore
use fuzzy_regex::FuzzyRegex;
use std::io::BufReader;
use std::fs::File;

let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
let mut stream = re.stream();

// Feed data in chunks
for m in stream.feed(b"hay") {
println ! ("Match at {}-{}", m.start(), m.end());
}
for m in stream.feed(b"stack needle here") {
println ! ("Match at {}-{}", m.start(), m.end());
}

// Or process a reader directly
let file = File::open("large_file.txt").unwrap();
for m in re.stream().search_reader(BufReader::new(file)) {
    println!("Match at byte offset {}", m.start());
}
```

### Byte-Level Search

```rust,ignore
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

// Search byte slices directly
if let Some(m) = re.find_bytes(b"hello world") {
    println!("Match at {}..{}", m.start(), m.end());
}

// Check if pattern supports fast streaming
if re.supports_streaming() {
println ! ("Pattern uses optimized Bitap algorithm");
}
```

### Case Insensitive Matching

```rust,ignore
use fuzzy_regex::FuzzyRegexBuilder;

let re = FuzzyRegexBuilder::new("(?:hello){e<=1}")
.case_insensitive(true)
.build()
.unwrap();

assert!(re.is_match("HELLO"));
assert!(re.is_match("HeLLo"));
assert!(re.is_match("HALLO")); // Case insensitive + fuzzy
```

### DNA Sequence Matching

```rust,ignore
use fuzzy_regex::FuzzyRegex;

// Find DNA sequences allowing for sequencing errors
let re = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();

let dna = "NNNNACGTACGTNNNN";
let m = re.find(dna).unwrap();
assert_eq!(m.as_str(), "ACGTACGT");

// Also matches with up to 2 errors
assert!(re.is_match("ACGTAGGT")); // 1 substitution
```

### Character Class Restrictions

Restrict which characters can be used for edits:

```rust,ignore
use fuzzy_regex::FuzzyRegex;

// Only allow lowercase letters as substitutions
let re = FuzzyRegex::new(r"(?:hello){s<=1:[a-z]}").unwrap();

assert!(re.is_match("hallo"));  // 'a' is in [a-z]
assert!(!re.is_match("h3llo")); // '3' is not in [a-z]
```

### Lookbehind

Match only after a specific pattern:

```rust,ignore
use fuzzy_regex::FuzzyRegex;

// Positive lookbehind - match "world" only when preceded by "hello "
let re = FuzzyRegex::new(r"(?<=hello )world").unwrap();

assert!(re.is_match("hello world"));
assert!(!re.is_match("bye world"));

// Negative lookbehind - match "world" only when NOT preceded by "hello "
let re = FuzzyRegex::new(r"(?<!hello )world").unwrap();

assert!(re.is_match("bye world"));
assert!(!re.is_match("hello world"));

// Variable-length lookbehind (fuzzy)
let re = FuzzyRegex::new(r"(?<=(?:hello){e<=1})world").unwrap();
assert!(re.is_match("hallo world")); // "hello" with 1 error before "world"
```

### Cost-Based Matching

Assign different costs to edit operations for fine-grained control:

```rust,ignore
use fuzzy_regex::FuzzyRegex;

// Simple cost: all operations cost 1, total cost <= 2
let re = FuzzyRegex::new("(?:hello){c<=2}").unwrap();
assert!(re.is_match("hallo")); // 1 sub, cost=1
assert!(re.is_match("helo"));  // 1 del, cost=1

// Weighted costs: insertions cost 2, others cost 1
let re = FuzzyRegex::new("(?:ab){2i+1d+1s+1t<=3}").unwrap();
assert!(re.is_match("abc")); // 1 ins, cost=2
assert!(re.is_match("a"));   // 1 del, cost=1
assert!(re.is_match("ba"));  // 1 transposition, cost=1
```

## Performance

### Algorithm Selection

The library automatically selects the best algorithm:

- **Bitap Algorithm**: Used for patterns ≤64 characters. O(n×k) time complexity where n is text length and k is max
  edits.
- **Levenshtein NFA**: Used for longer patterns or complex regex features.
- **DFA Fast Path**: For patterns without capture groups or lazy quantifiers.

### Optimization Tips

1. **Use specific edit limits**: `{e<=1}` is faster than `{e<=5}`
2. **Prefer shorter patterns**: Bitap is very fast for short patterns
3. **Use streaming for large texts**: Avoids loading entire file into memory
4. **Enable SIMD**: Enabled by default, provides ~2x speedup on supported platforms
5. **Consider mimalloc**: Enable the `mimalloc` feature for memory-intensive workloads

## API Overview

### Main Types

| Type                | Description                                 |
|---------------------|---------------------------------------------|
| `FuzzyRegex`        | Compiled regex pattern                      |
| `FuzzyRegexBuilder` | Builder for customized regex construction   |
| `Match`             | A single match with position and similarity |
| `Captures`          | Match with capture group information        |
| `StreamingMatcher`  | Stateful matcher for incremental processing |
| `StreamingMatch`    | Match result from streaming search          |

### Key Methods

```rust,ignore
// Construction
FuzzyRegex::new(pattern) -> Result<FuzzyRegex>
FuzzyRegex::builder(pattern) -> FuzzyRegexBuilder

// Searching
re.is_match(text) -> bool
re.find(text) -> Option<Match>
re.find_iter(text) -> impl Iterator<Item=Match>
re.captures(text) -> Option<Captures>

// Replacing
re.replace(text, replacement) -> String
re.replace_all(text, replacement) -> String

// Streaming
re.stream() -> StreamingMatcher
re.find_bytes(bytes) -> Option<StreamingMatch>
re.find_iter_bytes(bytes) -> impl Iterator<Item=StreamingMatch>

// Builder options
builder.similarity(threshold: f32)
builder.case_insensitive(yes: bool)
builder.multi_line(yes: bool)
builder.dot_all(yes: bool)
```

## Compatibility

This crate provides a compatibility layer for projects migrating from `fuzzy-aho-corasick`:

```rust,ignore
use fuzzy_regex::compat::fac::FuzzyAhoCorasickBuilder;
use fuzzy_regex::types::FuzzyLimits;

let searcher = FuzzyAhoCorasickBuilder::new()
    .fuzzy(FuzzyLimits::new().edits(1))
    .build(["hello", "world"])
    .unwrap();

for m in searcher.find_iter("helo wrld") {
    println!("Pattern {} matched", m.pattern_index());
}
```

## License

MIT License
