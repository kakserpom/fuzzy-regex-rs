# Error Messages

Understanding and handling errors.

## Pattern Errors

### Invalid Syntax

```text
Error: regex parse error
```

**Cause**: Malformed regex pattern.

**Fix**: Check for unclosed groups, invalid quantifiers, etc.

### Unsupported Feature

```text
Error: Feature not supported: backreferences
```

**Cause**: Pattern uses feature not implemented.

**Fix**: Simplify pattern or restructure.

### Empty Pattern

```text
Error: pattern cannot be empty
```

**Cause**: Empty regex not allowed.

**Fix**: Use non-empty pattern.

## Compilation Errors

### Fuzzy Limit Out of Range

```text
Error: edit limit too high
```

**Cause**: Edit limit exceeds maximum.

**Fix**: Reduce limit.

### Invalid Character Class

```text
Error: invalid character class
```

**Cause**: Malformed character class.

**Fix**: Check class syntax.

## Runtime Errors

### Timeout

```text
Error: timeout after ...
```

**Cause**: Matching took too long.

**Fix**: Simplify pattern, reduce edit limit.

### No Match

Not an error - use `find()` returning `Option`.

```rust,ignore
if let Some(m) = re.find(text) {
    // Match found
} else {
    // No match
}
```
