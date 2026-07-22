# Edit Distance

The edit distance (Damerau-Levenshtein distance) measures how many operations it takes to transform the pattern into the matched text.

## Edit Types

### Insertion

Adding a character to the text:

```rust,ignore
Pattern: "cat"
Text:    "cart" 
Edit:    Insert 'r' after 'c'
Cost:    1
```

### Deletion

Removing a character from the text:

```rust,ignore
Pattern: "cat"  
Text:    "ct"
Edit:    Delete 'a'
Cost:    1
```

### Substitution

Replacing one character with another:

```rust,ignore
Pattern: "cat"
Text:    "bat"
Edit:    Replace 'c' with 'b'
Cost:    1
```

### Transposition

Swapping two adjacent characters:

```rust,ignore
Pattern: "cat"
Text:    "act"
Edit:    Swap 'c' and 'a'
Cost:    1
```

## Accessing Edit Information

```rust
fn main() {
    use fuzzy_regex::FuzzyRegex;

    let re = FuzzyRegex::new("(?:hello){e<=2}").unwrap();
    let m = re.find("hallo").unwrap();

    println!("Similarity: {:.2}", m.similarity());
    println!("Edits: {}", m.len()); // Match length

    // Edit counts available in the engine
    // Use engine::EditCounts for detailed info
}
```

## Similarity Score

The similarity score ranges from 0.0 to 1.0, where 1.0 is an exact match:

```rust
fn main() {
    use fuzzy_regex::FuzzyRegexBuilder;

    let re = FuzzyRegexBuilder::new("(?:hello){e<=2}")
        .similarity(0.8) // Minimum similarity threshold
        .build()
        .unwrap();

    // Exact match: similarity = 1.0
    // 1 edit: similarity around 0.8-0.9
    // 2 edits: similarity around 0.6-0.8
    println!("{}", re.is_match("hello"));
    println!("{}", re.is_match("hallo"));
}
```
