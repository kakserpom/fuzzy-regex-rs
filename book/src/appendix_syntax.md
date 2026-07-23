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
| `\b`, `\B` | Word boundary / non-boundary |
| `\m`, `\M` | Start-of-word / end-of-word boundary |

## Character Escapes

| Syntax | Description |
|--------|-------------|
| `\n`, `\r`, `\t`, `\f`, `\v`, `\0` | Control characters |
| `\xHH` | Byte by hex code |
| `\u{HHHH}`, `\uHHHH` | Codepoint by hex |
| `\N{NAME}` | Character by Unicode name, e.g. `\N{BULLET}` |
| `\N{U+XXXX}` | Character by codepoint, e.g. `\N{U+1F600}` |

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
| `{Ni+Md...<=N}` | Weighted costs (coefficients optional: `{i+d+s<=N}`) |
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

## Recursion

| Syntax | Description |
|--------|-------------|
| `(?R)`, `(?0)` | Recursively match the whole pattern |
| `(?1)`, `(?2)`, … | Recursively match a numbered group |
| `(?&name)`, `(?P>name)` | Recursively match a named group |

Recursion enables patterns like balanced delimiters, e.g. `\((?:[^()]|(?R))*\)`.
Recursive patterns are matched by the backtracking engine. A recursion
reference may carry a fuzziness cap, e.g. `(?R){e<=2}`, limiting the total edits
in the recursive sub-match.

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
| `(?f)` | Full case folding (e.g. `ß` ↔ `ss`); only with `(?i)` |
