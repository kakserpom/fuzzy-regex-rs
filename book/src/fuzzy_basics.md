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
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // Allow up to 2 total edits
    let re1 = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

    // Allow specific edit types
    let re2 = FuzzyRegex::new("(?:hello){i<=1,d<=1}").unwrap();

    // Allow substitutions only
    let re3 = FuzzyRegex::new("(?:hello){s<=2}").unwrap();
    
    println!("re1: {:?}", re1.is_match("hello"));
    println!("re2: {:?}", re2.is_match("helo"));
    println!("re3: {:?}", re3.is_match("hallo"));
}
```

## Shorthand Syntax

Use `~N` as shorthand for `{e<=N}`:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    // These are equivalent:
    let re1 = FuzzyRegex::new("(?:hello)~2").unwrap();
    let re2 = FuzzyRegex::new("(?:hello){e<=2}").unwrap();

    // Exact match with ~0
    let re3 = FuzzyRegex::new("(?:hello)~0").unwrap();
    
    println!("re1 == re2: {}", re1.is_match("hello") == re2.is_match("hello"));
    println!("re3 exact: {}", re3.is_match("hello"));
}
```

## Unlimited Errors

Omit the number to allow unlimited edits:

```rust,ignore
fn main() {
    // Allow unlimited substitutions
    let re = FuzzyRegex::new("(?:hello){s}").unwrap();

    // Allow any number of errors
    let re2 = FuzzyRegex::new("(?:hello){e}").unwrap();
    
    println!("{}", re.is_match("hallo"));
    println!("{}", re2.is_match("xyz"));
}
```
