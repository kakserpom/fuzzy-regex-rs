//! Tests for `\N{...}` named-Unicode escapes: `\N{NAME}` (Unicode character
//! name, backed by the `unicode-names2` database) and `\N{U+XXXX}` (codepoint).

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn named_escape_resolves_to_character() {
    let re = FuzzyRegex::new(r"\N{LATIN SMALL LETTER SHARP S}").unwrap();
    assert_eq!(span(&re, "ß"), Some((0, 2))); // ß is 2 UTF-8 bytes
    assert!(re.find("x").is_none());
}

#[test]
fn named_escape_in_context() {
    let re = FuzzyRegex::new(r"a\N{BULLET}b").unwrap();
    assert_eq!(span(&re, "a•b"), Some((0, 5)));
}

#[test]
fn named_escape_with_quantifier() {
    let re = FuzzyRegex::new(r"\N{GREEK SMALL LETTER ALPHA}+").unwrap();
    assert_eq!(span(&re, "ααα"), Some((0, 6)));
}

#[test]
fn named_escape_in_character_class() {
    let re = FuzzyRegex::new(r"[\N{BULLET}x]+").unwrap();
    assert_eq!(span(&re, "x•x"), Some((0, 5)));
}

#[test]
fn codepoint_form() {
    assert_eq!(
        span(&FuzzyRegex::new(r"\N{U+0041}").unwrap(), "A"),
        Some((0, 1))
    );
    // Astral-plane codepoint (4 UTF-8 bytes).
    assert_eq!(
        span(&FuzzyRegex::new(r"\N{U+1F600}").unwrap(), "😀"),
        Some((0, 4))
    );
    // Lowercase `u+` accepted too.
    assert_eq!(
        span(&FuzzyRegex::new(r"\N{u+0041}").unwrap(), "A"),
        Some((0, 1))
    );
}

#[test]
fn named_escape_with_fuzziness() {
    // Corpus L4448 shape: fuzzy over a \N{} char (edit-char restriction).
    let re = FuzzyRegex::new(r"(?fiu)(?:\N{LATIN SMALL LETTER SHARP S}){e<=1:[a-z]}").unwrap();
    assert_eq!(span(&re, "ß"), Some((0, 2)));
}

#[test]
fn errors() {
    assert!(FuzzyRegex::new(r"\N{NOT A REAL NAME}").is_err()); // unknown name
    assert!(FuzzyRegex::new(r"\N").is_err()); // no brace
    assert!(FuzzyRegex::new(r"\N{}").is_err()); // empty
    assert!(FuzzyRegex::new(r"\N{BULLET").is_err()); // unclosed
    assert!(FuzzyRegex::new(r"\N{U+110000}").is_err()); // out-of-range codepoint
}
