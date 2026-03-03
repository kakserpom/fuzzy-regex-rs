# Contributing to fuzzy-regex

## Development Setup

```bash
# Run tests
cargo test --all-features

# Run linter (required before commit)
cargo clippy --all-features -- -D warnings

# Format code (required before commit)
cargo fmt --all
```

## Commit Message Style

Use release-plz format:
- `feat:` - New features
- `fix:` - Bug fixes
- `perf:` - Performance improvements
- `docs:` - Documentation
- `refactor:` - Code restructuring
- Example: `perf: add optimization for .*SUFFIX patterns`

## Pull Request Checklist

- [ ] All tests pass: `cargo test --all-features`
- [ ] Clippy passes: `cargo clippy --all-features -- -D warnings`
- [ ] Code is formatted: `cargo fmt --all`
- [ ] Tests for new functionality are included
- [ ] Book is updated if applicable (see /book directory)
