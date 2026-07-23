# Pattern Syntax Reference

Complete reference for all pattern syntax.

## Special Characters

| Character | Description |
|-----------|-------------|
| `.` | Any character except newline |
| `^` | Start of string |
| `$` | End of string |
| `|` | Alternation |
| `\` | Escape |
| `(`, `)` | Groups |
| `[`, `]` | Character class |
| `{`, `}` | Quantifier or fuzziness |

## Character Classes

| Syntax | Description |
|--------|-------------|
| `[abc]` | a, b, or c |
| `[^abc]` | Not a, b, or c |
| `[a-z]` | a to z |
| `[A-Za-z]` | ASCII letters |
| `[0-9]` | Digits |
| `\d` | Digit |
| `\D` | Non-digit |
| `\w` | Word character |
| `\W` | Non-word |
| `\s` | Whitespace |
| `\S` | Non-whitespace |

## Quantifiers

| Syntax | Description |
|--------|-------------|
| `*` | 0 or more |
| `+` | 1 or more |
| `?` | 0 or 1 |
| `{n}` | Exactly n |
| `{n,}` | n or more |
| `{n,m}` | n to m |
| `*?` | Lazy * |
| `+?` | Lazy + |
| `??` | Lazy ? |

## Fuzziness Markers

| Syntax | Description |
|--------|-------------|
| `{e<=N}` | Max N edits |
| `{i<=N}` | Max N insertions |
| `{d<=N}` | Max N deletions |
| `{s<=N}` | Max N substitutions |
| `{t<=N}` | Max N transpositions |
| `{c<=N}` | Max total cost N |
| `{Ni+Md...<=N}` | Weighted costs |
| `{e<=N:[class]}` | Restricted edits |
| `~N` | Shorthand for {e<=N} |

## Groups

| Syntax | Description |
|--------|-------------|
| `(...)` | Capture group |
| `(?:...)` | Non-capture |
| `(?<name>...)`, `(?P<name>...)` | Named capture |
| `(?=...)` | Lookahead |
| `(?!...)` | Negative lookahead |
| `(?<=...)` | Lookbehind |
| `(?<!...)` | Negative lookbehind |
| `(?>...)` | Atomic group |

## Backreferences

| Syntax | Description |
|--------|-------------|
| `\1` … `\9` | Backreference to a numbered group |
| `\k<name>`, `\k{name}` | Backreference to a named group |
| `(?P=name)` | Backreference to a named group (Python style) |

Backreferences accept a fuzziness suffix, e.g. `\1{e<=1}` or `\k<w>{s<=1}`,
matching a repeat of the captured text within the given edit budget.

## Flags

| Syntax | Description |
|--------|-------------|
| `(?i)` | Case insensitive |
| `(?m)` | Multi-line |
| `(?s)` | Dot-all |
| `(?x)` | Verbose |
| `(?U)` | Ungreedy |
| `(?b)` | Best match |
| `(?e)` | Enhance match |
| `(?p)` | POSIX mode |
| `(?r)` | Reverse (search from the end; rightmost match) |
