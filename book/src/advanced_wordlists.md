# Word Lists

`\L<name>` matches any string from a named list. The list is a set of literal
words (not sub-patterns); `\L<name>` behaves like an alternation of those words,
`(?:word1|word2|...)`.

## Basic Usage

Compile the pattern, then attach the list with `set_word_list(name, words)`:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let mut re = FuzzyRegex::new(r"\L<keywords>").unwrap();
    re.set_word_list("keywords", vec!["hello", "world", "test"]);

    assert!(re.is_match("hello"));
    assert!(re.is_match("world"));
    assert!(!re.is_match("foo"));
}
```

The name in the pattern (`keywords`) must match the name passed to
`set_word_list`. `set_word_list` takes the list name and a `Vec` of words and
returns `()`.

## Resolution is deferred

`\L<name>` compiles before the list is known and is resolved later by
`set_word_list`. Until a referenced list is provided (or if it is set to an
empty list), that reference is an empty alternation and **matches nothing** — not
the empty string:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new(r"\L<keywords>").unwrap();
    // No list set yet:
    assert!(!re.is_match("hello"));
    assert!(re.find("hello").is_none());
}
```

Because the list is expanded into the engine like any other alternation, it
composes normally with the rest of the pattern — surrounding anchors and word
boundaries are honored, it can appear inside a larger pattern, and capture groups
around it work:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // `\b` boundaries are honored: "dog" must be a whole word.
    let mut re = FuzzyRegex::new(r"\b\L<w>\b").unwrap();
    re.set_word_list("w", vec!["cat", "dog"]);
    assert_eq!(re.find("a dog x").map(|m| (m.start(), m.end())), Some((2, 5)));
    assert!(re.find("adogx").is_none()); // no word boundary

    // Embedded in a larger pattern, with a capture group:
    let mut kv = FuzzyRegex::new(r"(\w+)=\L<v>").unwrap();
    kv.set_word_list("v", vec!["on", "off"]);
    let caps = kv.captures("mode=off").unwrap();
    assert_eq!(caps.get(1).unwrap().as_str(), "mode");
}
```

## With Fuzzy Matching

Attach a fuzzy budget to the reference to match words approximately:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let mut re = FuzzyRegex::new(r"\L<keywords>{e<=1}").unwrap();
    re.set_word_list("keywords", vec!["hello", "world"]);

    assert!(re.is_match("hallo")); // "hello" with 1 substitution
    assert!(re.is_match("helo"));  // "hello" with 1 deletion
    assert!(!re.is_match("xyz"));
}
```

The budget uses the same edit metric as the rest of the crate (`{e<=n}`,
`{s<=n}`, etc.). Note that a transposition counts as two edits under `e` unless
you allow a swap explicitly (`{t<=1}`), so `"wrold"` does **not** match
`world{e<=1}`.

## Multiple Word Lists

A pattern may reference several lists; set each one. Every referenced list must
be resolved before the pattern matches — while any is still unset, the whole
pattern matches nothing (see [Resolution is deferred](#resolution-is-deferred)):

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let mut re = FuzzyRegex::new(r"\b\L<names>\b|\b\L<places>\b").unwrap();
    re.set_word_list("names", vec!["alice", "bob"]);
    re.set_word_list("places", vec!["paris", "rome"]);

    assert!(re.is_match("alice"));
    assert!(re.is_match("paris"));
}
```

## Large lists: the Aho-Corasick fast path

Expanding a huge list into an alternation is slow to scan (the engine tries each
word). When the pattern is a *pure word-list* reference — a single `\L<name>`
(optionally fuzzy) wrapped only in anchors (`^`/`$`) and/or word boundaries
(`\b`), with no capture groups — and the resolved list is large, fuzzy-regex
matches it with an [Aho-Corasick](https://github.com/kakserpom/fuzzy-aho-corasick-rs)
automaton in a single pass instead. Results are identical to the alternation;
only the speed differs (dramatically for large lists — see
[Performance Tips](perf_tips.md)).

This is controlled by the `word-list-ac` Cargo feature (enabled by default) and a
size threshold you can tune on the builder:

```rust
fn main() {
    use fuzzy_regex::{FuzzyRegexBuilder, DEFAULT_WORD_LIST_AC_THRESHOLD};

    // Default threshold (DEFAULT_WORD_LIST_AC_THRESHOLD == 64): lists larger than
    // this use the automaton; smaller ones use the NFA alternation.
    let _ = DEFAULT_WORD_LIST_AC_THRESHOLD;

    let mut re = FuzzyRegexBuilder::new(r"\b\L<w>\b")
        .word_list_ac_threshold(1000) // only lists with >1000 words use the automaton
        .build()
        .unwrap();
    re.set_word_list("w", vec!["cat", "dog"]);
    assert!(re.is_match("cat"));
}
```

Set the threshold very high to keep every list on the NFA, or low to engage the
automaton sooner. The choice never changes results, only performance.

## Use Cases

- **Keyword matching**: match against a list of keywords
- **Name matching**: match against a database of names
- **Dictionary lookup**: match words from a dictionary (large lists benefit from
  the Aho-Corasick fast path above)
