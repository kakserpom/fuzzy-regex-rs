# Fuzzy Matching Basics

Fuzzy matching allows matching strings that are "close" to the pattern, not just exact matches. The closeness is measured using **edit distance**.

## Edit Distance

Edit distance is the minimum number of character-level operations needed to transform one string into another:

- **Insertion**: Add a character (cost: 1)
- **Deletion**: Remove a character (cost: 1)
- **Substitution**: Replace one character with another (cost: 1)
- **Transposition**: Swap two adjacent characters (cost: 1)

Example: "hello" → "hallo" (1 substitution)

## Fuzziness Markers

Apply fuzziness to a pattern segment using `{...}`:

| Syntax | Description |
|--------|-------------|
| `(?:text){e<=N}` | Allow up to N total edits |
| `(?:text){i<=N}` | Allow up to N insertions |
| `(?:text){d<=N}` | Allow up to N deletions |
| `(?:text){s<=N}` | Allow up to N substitutions |
| `(?:text){t<=N}` | Allow up to N transpositions |

```rust
use fuzzy_regex::FuzzyRegex;

// Allow up to 2 total edits
let re1 = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

// Allow specific edit types
let re2 = FuzzyRegex::new("(?:hello){i<=1,d<=1}").unwrap();

// Allow substitutions only
let re3 = FuzzyRegex::new("(?:hello){s<=2}").unwrap();
```

## Shorthand Syntax

Use `~N` as shorthand for `{e<=N}`:

```rust
use fuzzy_regex::FuzzyRegex;

// These are equivalent:
let re1 = FuzzyRegex::new("(?:hello)~2").unwrap();
let re2 = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

// Exact match with ~0
let re3 = FuzzyRegex::new("(?:hello)~0").unwrap();
```

## Unlimited Errors

Omit the number to allow unlimited edits:

```rust
// Allow unlimited substitutions
let re = FuzzyRegex::new("(?:hello){s}").unwrap();

// Allow any number of errors
let re2 = FuzzyRegex::new("(?:hello){e}").unwrap();
```
