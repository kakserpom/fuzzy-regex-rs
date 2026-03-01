# Capture Groups

Extract specific portions of matches.

## Named Capture Groups

```rust
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new(r"(?<user>\w+)@(?<domain>\w+\.\w+)").unwrap();
let caps = re.captures("john@example.com").unwrap();

assert_eq!(caps.name("user").unwrap().as_str(), "john");
assert_eq!(caps.name("domain").unwrap().as_str(), "example.com");
```

## Numbered Capture Groups

```rust
let re = FuzzyRegex::new(r"(\w+)@(\w+)").unwrap();
let caps = re.captures("test@example").unwrap();

assert_eq!(caps.get(1).unwrap().as_str(), "test");
assert_eq!(caps.get(2).unwrap().as_str(), "example");
assert_eq!(caps.get(0).unwrap().as_str(), "test@example"); // Full match
```

## Nested Groups

```rust
let re = FuzzyRegex::new(r"((?P<outer>\w+)-(?P<inner>\w+))").unwrap();
let caps = re.captures("hello-world").unwrap();

assert_eq!(caps.get(0).unwrap().as_str(), "hello-world");
assert_eq!(caps.get(1).unwrap().as_str(), "hello-world");
assert_eq!(caps.name("outer").unwrap().as_str(), "hello");
assert_eq!(caps.name("inner").unwrap().as_str(), "world");
```

## Capture Groups with Fuzzy Matching

```rust
use fuzzy_regex::FuzzyRegex;

let re = FuzzyRegex::new(r"(?<word>\w+){e<=1}").unwrap();
let caps = re.captures("tset").unwrap();

assert_eq!(caps.name("word").unwrap().as_str(), "tset");
```

## Iterating Captures

```rust
let re = FuzzyRegex::new(r"(\w+)").unwrap();
let caps = re.captures("hello world").unwrap();

for cap in caps.iter() {
    if let Some(m) = cap {
        println!("Group: '{}'", m.as_str());
    }
}
```

## Accessing All Groups

```rust
let re = FuzzyRegex::new(r"(\w+)@(\w+)").unwrap();
let caps = re.captures("a@b").unwrap();

assert_eq!(caps.len(), 3); // 0 (full) + 2 groups

// Names of capture groups
for name in caps.iter_names() {
    if let Some(n) = name {
        println!("Group name: {}", n);
    }
}
```
