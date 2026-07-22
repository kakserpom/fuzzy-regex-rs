# API Reference

Overview of the main API types and methods.

## Main Types

| Type | Description |
|------|-------------|
| `FuzzyRegex` | Compiled regex pattern |
| `FuzzyRegexBuilder` | Builder for customizing regex |
| `Match` | A single match result |
| `Captures` | Match with capture groups |
| `StreamingMatcher` | Stateful matcher for streaming |
| `StreamingMatch` | Match from streaming search |

## Quick Method Reference

```rust,ignore
use fuzzy_regex::FuzzyRegex;

// Construction
let re = FuzzyRegex::new("pattern").unwrap();

// Searching
re.is_match(text)           // bool - does it match?
re.find(text)               // Option<Match>
re.find_iter(text)          // Iterator<Match>
re.find_rev(text)           // Option<Match> - rightmost
re.captures(text)           // Option<Captures>

// Replacing
re.replace(text, "x")       // String
re.replace_all(text, "x")   // String
re.split(text)              // Split

// Streaming
re.stream()                 // StreamingMatcher
re.find_bytes(bytes)        // Option<StreamingMatch>
```
