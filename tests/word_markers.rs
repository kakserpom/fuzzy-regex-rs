//! Tests for the `\m` (start-of-word) and `\M` (end-of-word) boundary markers,
//! the directional halves of `\b`.

use fuzzy_regex::FuzzyRegex;

fn span(re: &FuzzyRegex, text: &str) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[test]
fn word_start_marker() {
    let re = FuzzyRegex::new(r"\mword").unwrap();
    assert_eq!(span(&re, "a word here"), Some((2, 6))); // "word" starts a word
    assert!(re.find("sword here").is_none()); // "word" inside "sword" is not a start
    assert_eq!(span(&re, "word up"), Some((0, 4))); // start of string
}

#[test]
fn word_end_marker() {
    let re = FuzzyRegex::new(r"word\M").unwrap();
    assert_eq!(span(&re, "a word here"), Some((2, 6))); // "word" ends a word
    assert!(re.find("a words here").is_none()); // "word" inside "words" is not an end
    assert_eq!(span(&re, "the word"), Some((4, 8))); // end of string
}

#[test]
fn whole_word_with_both_markers() {
    let re = FuzzyRegex::new(r"\mfoo\M").unwrap();
    assert_eq!(span(&re, "a foo bar"), Some((2, 5)));
    assert!(re.find("a foobar").is_none()); // not a whole word
    assert!(re.find("barfoo").is_none());
}

#[test]
fn markers_with_classes() {
    assert_eq!(
        span(&FuzzyRegex::new(r"\m\w+").unwrap(), "  hello"),
        Some((2, 7))
    );
    assert_eq!(
        span(&FuzzyRegex::new(r"\w+\M").unwrap(), "hello  "),
        Some((0, 5))
    );
}

#[test]
fn markers_with_fuzziness() {
    // A fuzzy word constrained to whole-word boundaries.
    let re = FuzzyRegex::new(r"\m(?:word){e<=2}\M").unwrap();
    assert_eq!(span(&re, "a wodr here"), Some((2, 6))); // transposition, whole word
    assert!(re.find("a swordfish").is_none()); // not whole-word bounded
}

#[test]
fn markers_inside_lookbehind_compile_and_match() {
    // `\m`/`\M` are accepted inside lookaround (corpus L654/L657 shape).
    let re = FuzzyRegex::new(r"foo(?<=\mfoo\M)").unwrap();
    assert_eq!(span(&re, "foo"), Some((0, 3)));
    // A word-boundary marker in a lookbehind at least compiles and runs in a
    // fuzzy + negative-lookbehind combination (the corpus pattern shape).
    let corpus = FuzzyRegex::new(r"(?iV0)\m(?:word){e<=3}\M(?<!\m(?:word){e<=1}\M)").unwrap();
    let _ = corpus.find("a wxrd here");
}

#[test]
fn not_valid_in_char_class() {
    assert!(FuzzyRegex::new(r"[\m]").is_err());
    assert!(FuzzyRegex::new(r"[\M]").is_err());
}

#[test]
fn version_flag_accepted() {
    // mrab `(?V0)`/`(?V1)` version flags parse (no behavioural effect here).
    assert!(FuzzyRegex::new(r"(?V0)abc").is_ok());
    assert!(FuzzyRegex::new(r"(?iV1)\mword\M").is_ok());
}
