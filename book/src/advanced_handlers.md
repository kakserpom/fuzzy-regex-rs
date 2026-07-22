# Custom Handlers

Custom handlers allow you to invoke arbitrary Rust code during regex matching. This is useful for patterns that are difficult or impossible to express with regular regex alone.

## Syntax

Use `(?call:handler_name)` in your pattern to invoke a handler:

```rust,ignore
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new("(?call:myhandler)")
    .handler("myhandler", |text, pos| {
        // Your logic here
    })
    .build()
    .unwrap();
```

## Handler Signature

Handlers have the signature:

```rust,ignore
Fn(&str, usize) -> HandlerResult
```

- **`text`**: The entire input string being matched
- **`pos`**: Current position in the text (byte index)
- **`HandlerResult::MatchOverride(n, text)`**: Handler matched, consume `n` bytes, optionally override captured text
- **`HandlerResult::NoMatch`**: Handler didn't match at this position

> **Important**: The byte count must account for UTF-8 encoding. For multi-byte characters (like Cyrillic, emoji), use byte length, not character count.

## Examples

### Simple Matching

Match strings that start with a specific prefix:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new(r"(?call:prefix)")
    .handler("prefix", |text, pos| {
        if text[pos..].starts_with("foo") {
            HandlerResult::MatchOverride(3, "FOO".to_string())
        } else {
            HandlerResult::NoMatch
        }
    })
    .build()
    .unwrap();

let m = re.find("foobar").unwrap();
assert_eq!(m.as_str(), "foo");
```

### Capture Override

Handlers can override the captured text while still matching the original:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new(r"(prefix(?call:handler)suffix)")
    .handler("handler", |text, pos| {
        if text[pos..].starts_with("XYZ") {
            // Match 3 bytes but capture as lowercase
            HandlerResult::MatchOverride(3, "xyz".to_string())
        } else {
            HandlerResult::NoMatch
        }
    })
    .build()
    .unwrap();

let caps = re.captures("prefixXYZsuffix").unwrap();
// Captured text is "xyz" (the override), not "XYZ"
assert_eq!(caps.get(1).unwrap().as_str(), "prefixxyzsuffix");
```

### Matching Escaped Characters

Match strings with escaped quotes:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new(r#""((?call:unescape)|.)*""#)
    .handler("unescape", |text, pos| {
        if pos + 1 < text.len() && text.as_bytes()[pos] == b'\\' {
            HandlerResult::MatchOverride(2, String::new()) // consume \ and next char
        } else {
            HandlerResult::NoMatch
        }
    })
    .build()
    .unwrap();

let m = re.find(r#""hello \"world\"""#).unwrap();
assert_eq!(m.as_str(), "\"hello \\\"world\\\"\"");

let caps = re.captures(r#""hello \"world\"""#).unwrap();
// The handler_overrides contain the override information
println!("{:?}", caps.handler_overrides());
```

### Escape Sequences

Match common escape sequences like `\n`, `\t`, `\\`:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new(r"(?call:escape)+")
    .handler("escape", |text, pos| {
        if pos + 1 < text.len() && text.as_bytes()[pos] == b'\\' {
            let next = text.as_bytes()[pos + 1];
            if matches!(next, b'n' | b't' | b'r' | b'\\' | b'"' | b'\'') {
                return HandlerResult::MatchOverride(2, String::new());
            }
        }
        HandlerResult::NoMatch
    })
    .build()
    .unwrap();

re.find(r"hello\nworld"); // matches "\n"
re.find(r"a\tb");         // matches "\t"
```

### Context-Sensitive Matching

Match patterns that depend on context:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

// Match "id:123" only when preceded by "user:"
let re = FuzzyRegexBuilder::new(r"user:(?call:id)")
    .handler("id", |text, pos| {
        if text[pos..].starts_with("id:") {
            let rest = &text[pos + 3..];
            let mut count = 0;
            for (_, c) in rest.char_indices() {
                if c.is_ascii_digit() {
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 {
                return HandlerResult::MatchOverride(3 + count, String::new());
            }
        }
        HandlerResult::NoMatch
    })
    .build()
    .unwrap();

re.find("user:id:123"); // matches "user:id:123"
re.find("user:name:foo"); // no match (not "id:")
re.find("id:999"); // no match (no "user:" prefix)
```

### Unicode Text Transformation

Handle non-ASCII text (note: byte count matters for UTF-8):

```rust,ignore
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new(r#""(?call:translate)""#)
    .handler("translate", |text, pos| {
        let remaining = &text[pos..];
        // "привет" is 6 Cyrillic chars = 12 bytes in UTF-8
        if remaining.starts_with("привет") {
            HandlerResult::MatchOverride(12, "HELLO".to_string())
        } else {
            HandlerResult::NoMatch
        }
    })
    .build()
    .unwrap();

let caps = re.captures("\"привет\"").unwrap();
// The captured text shows the override
assert_eq!(caps.get(0).unwrap().as_str(), "\"HELLO\"");

// Handler overrides track (start_byte, end_byte, override_text)
assert_eq!(caps.handler_overrides(), &[(1, 13, "HELLO")]);
```

### Fuzzy Matching with Handlers

Combine handlers with fuzzy matching for powerful patterns:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

// Fuzzy lookbehind + handler for complex validation
let re = FuzzyRegexBuilder::new(r"(?<=(?:hello){e<=2}) (?call:translate)")
    .handler("translate", |text, pos| {
        let remaining = &text[pos..];
        // "привет" = 12 bytes
        if remaining.starts_with("привет") {
            HandlerResult::MatchOverride(12, "HELLO".to_string())
        } else {
            HandlerResult::NoMatch
        }
    })
    .build()
    .unwrap();

// Matches with fuzzy lookbehind
let m = re.find("hello привет").unwrap();
assert_eq!(m.as_str(), " привет");

// Also matches with fuzzy lookbehind (1 extra 'l')
let m = re.find("helllo привет").unwrap();
assert_eq!(m.as_str(), " привет");
```

### Multiple Handlers

Use multiple handlers in the same pattern:

```rust,ignore
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

let re = FuzzyRegexBuilder::new(r"(?call:foo).*(?call:bar)")
    .handler("foo", |text, pos| {
        if text[pos..].starts_with("START") {
            HandlerResult::MatchOverride(5, "start".to_string())
        } else {
            HandlerResult::NoMatch
        }
    })
    .handler("bar", |text, pos| {
        if text[pos..].starts_with("END") {
            HandlerResult::MatchOverride(3, "end".to_string())
        } else {
            HandlerResult::NoMatch
        }
    })
    .build()
    .unwrap();

let caps = re.captures("START middle END").unwrap();
assert_eq!(caps.get(0).unwrap().as_str(), "START middle END");
```

### Validation Handlers

Use handlers to implement complex validation rules:

```rust
use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

// Match email-like patterns but validate the domain
let re = FuzzyRegexBuilder::new(r"\w+@(?call:domain)")
    .handler("domain", |text, pos| {
        // Find the end of the domain (until whitespace or end)
        let remaining = &text[pos..];
        let mut end = 0;
        for (i, c) in remaining.char_indices() {
            if c.is_whitespace() {
                break;
            }
            end = i + c.len_utf8();
        }
        if end > 0 {
            let domain = &remaining[..end];
            // Only allow .com, .org, .net domains
            if domain.ends_with(".com") || domain.ends_with(".org") || domain.ends_with(".net") {
                return HandlerResult::MatchOverride(end, String::new());
            }
        }
        HandlerResult::NoMatch
    })
    .build()
    .unwrap();

re.find("user@example.com"); // matches
re.find("user@test.org");   // matches
re.find("user@fake.io");    // no match (invalid TLD)
```

## Performance Notes

- Handlers are called during NFA simulation, which may be slower than optimized paths
- For best performance, keep handler logic simple
- Consider using handlers only when necessary; standard regex is faster for expressible patterns
- When using `MatchOverride`, the override text is stored separately and applied during capture construction

## Limitations

- Handlers cannot perform lookahead/lookbehind themselves
- Handler matches are exact (not fuzzy) - they either match or don't
- Captures inside handlers are limited
- Position is in bytes, not characters - account for UTF-8 encoding
