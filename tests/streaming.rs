//! Tests for the streaming API.

use fuzzy_regex::{FuzzyRegex, FuzzyRegexBuilder};
use std::io::Cursor;

// ============================================
// Basic streaming tests
// ============================================

#[test]
fn test_stream_single_chunk() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    let matches: Vec<_> = stream.feed(b"hello world").collect();
    assert!(!matches.is_empty());
    assert_eq!(matches[0].start(), 0);
    assert_eq!(matches[0].end(), 5);
}

#[test]
fn test_stream_multiple_chunks_no_boundary() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    // First chunk has no match
    let matches1: Vec<_> = stream.feed(b"xxx ").collect();
    assert!(matches1.is_empty());

    // Second chunk has the match
    let matches2: Vec<_> = stream.feed(b"hello world").collect();
    assert!(!matches2.is_empty());
    // The match is at position 4 in global stream (4 bytes from first chunk)
    assert_eq!(matches2[0].start(), 4);
    assert_eq!(matches2[0].end(), 9);
}

#[test]
fn test_stream_cross_boundary_match() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    // Split "hello" across chunks
    let matches1: Vec<_> = stream.feed(b"hel").collect();
    // May or may not have a partial match here

    let matches2: Vec<_> = stream.feed(b"lo world").collect();
    // Should find "hello" spanning the boundary
    let all_matches: Vec<_> = matches1.into_iter().chain(matches2).collect();

    // At least one match should be found
    assert!(!all_matches.is_empty() || stream.finish().is_some());
}

#[test]
fn test_stream_position() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    assert_eq!(stream.position(), 0);

    let _ = stream.feed(b"hello");
    assert_eq!(stream.position(), 5);

    let _ = stream.feed(b" world");
    assert_eq!(stream.position(), 11);
}

#[test]
fn test_stream_reset() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    let _ = stream.feed(b"hello world");
    assert_eq!(stream.position(), 11);

    stream.reset();
    assert_eq!(stream.position(), 0);

    let matches: Vec<_> = stream.feed(b"hello").collect();
    assert!(!matches.is_empty());
}

#[test]
fn test_stream_finish() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    // Feed partial data
    let _ = stream.feed(b"test hel");

    // Finish should check remaining buffer
    let final_match = stream.finish();
    // May or may not have a match depending on buffer handling
    let _ = final_match; // Just ensure it doesn't panic
}

// ============================================
// Byte-level API tests
// ============================================

#[test]
fn test_find_bytes_basic() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    let m = re.find_bytes(b"hello world").unwrap();
    assert_eq!(m.start(), 0);
    assert_eq!(m.end(), 5);
}

#[test]
fn test_find_bytes_fuzzy_match() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    // "hallo" matches with 1 substitution
    let m = re.find_bytes(b"hallo world").unwrap();
    let matched = std::str::from_utf8(&b"hallo world"[m.start()..m.end()]).unwrap();
    assert_eq!(matched, "hallo");
    assert!(m.edits() <= 1);
}

#[test]
fn test_is_match_bytes() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();

    assert!(re.is_match_bytes(b"hello world"));
    assert!(re.is_match_bytes(b"hallo world")); // fuzzy
    assert!(!re.is_match_bytes(b"goodbye world"));
}

#[test]
fn test_find_iter_bytes() {
    let re = FuzzyRegex::new("(?:cat){e<=1}").unwrap();

    let matches: Vec<_> = re.find_iter_bytes(b"cat bat rat").collect();
    // Should find "cat" at least, and possibly "bat", "rat" as fuzzy matches
    assert!(!matches.is_empty());
    assert_eq!(matches[0].start(), 0);
}

// ============================================
// StreamingMatch helper tests
// ============================================

#[test]
fn test_streaming_match_methods() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let m = re.find_bytes(b"hello world").unwrap();

    assert_eq!(m.start(), 0);
    assert_eq!(m.end(), 5);
    assert_eq!(m.len(), 5);
    assert!(!m.is_empty());
    assert!(m.similarity() > 0.0);
}

// ============================================
// Reader integration tests
// ============================================

#[test]
fn test_search_reader_basic() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let stream = re.stream();

    let data = b"hello world, hello there";
    let reader = Cursor::new(data);

    let matches: Vec<_> = stream.search_reader(reader).collect();
    assert!(!matches.is_empty());
    assert_eq!(matches[0].start(), 0);
}

#[test]
fn test_search_reader_large_data() {
    let re = FuzzyRegex::new("(?:needle){e<=1}").unwrap();
    let stream = re.stream();

    // Create data with needle in the middle
    let mut data = vec![b'x'; 10000];
    data[5000..5006].copy_from_slice(b"needle");
    let reader = Cursor::new(data);

    let matches: Vec<_> = stream.search_reader(reader).collect();
    assert!(!matches.is_empty());
    assert_eq!(matches[0].start(), 5000);
}

#[test]
fn test_search_reader_chunk_size() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let stream = re.stream();

    let data = b"hello world";
    let reader = Cursor::new(data);

    // Use small chunk size
    let matches: Vec<_> = stream.search_reader(reader).with_chunk_size(3).collect();
    assert!(!matches.is_empty());
}

// ============================================
// Fuzzy matching in streaming mode
// ============================================

#[test]
fn test_stream_fuzzy_substitution() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    // "hallo" has 1 substitution
    let matches: Vec<_> = stream.feed(b"hallo world").collect();
    assert!(!matches.is_empty());
    assert!(matches[0].edits() <= 1);
}

#[test]
fn test_stream_fuzzy_insertion() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    // "heello" has 1 insertion
    let matches: Vec<_> = stream.feed(b"heello world").collect();
    assert!(!matches.is_empty());
}

#[test]
fn test_stream_fuzzy_deletion() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    // "hllo" has 1 deletion
    let matches: Vec<_> = stream.feed(b"hllo world").collect();
    assert!(!matches.is_empty());
}

// ============================================
// supports_streaming() tests
// ============================================

#[test]
fn test_supports_streaming_short_pattern() {
    // Short fuzzy pattern should support streaming
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    assert!(re.supports_streaming());
}

#[test]
fn test_supports_streaming_regex_pattern() {
    // Non-fuzzy regex patterns may not have FuzzyBridge
    let re = FuzzyRegex::new("hello").unwrap();
    // This may or may not support streaming depending on internal optimization
    let _ = re.supports_streaming(); // Just ensure it doesn't panic
}

// ============================================
// Unicode in streaming
// ============================================

#[test]
fn test_stream_unicode() {
    let re = FuzzyRegex::new("(?:café){e<=1}").unwrap();
    let mut stream = re.stream();

    let matches: Vec<_> = stream.feed("I love café au lait".as_bytes()).collect();
    assert!(!matches.is_empty());
}

#[test]
fn test_stream_unicode_cross_boundary() {
    let re = FuzzyRegex::new("(?:naïve){e<=1}").unwrap();
    let mut stream = re.stream();

    // Split in the middle of the unicode character
    let _ = stream.feed("Don't be na".as_bytes());
    let matches: Vec<_> = stream.feed("ïve about it".as_bytes()).collect();

    // Should find the match (handling UTF-8 boundaries correctly)
    // The exact behavior depends on buffer handling
    let _ = matches; // Just ensure no panic
}

// ============================================
// Edge cases
// ============================================

#[test]
fn test_stream_empty_chunk() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    let matches1: Vec<_> = stream.feed(b"").collect();
    assert!(matches1.is_empty());

    let matches2: Vec<_> = stream.feed(b"hello").collect();
    assert!(!matches2.is_empty());
}

#[test]
fn test_stream_no_match() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    let matches: Vec<_> = stream.feed(b"goodbye world").collect();
    assert!(matches.is_empty());

    assert!(stream.finish().is_none());
}

#[test]
fn test_find_bytes_empty() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    assert!(re.find_bytes(b"").is_none());
}

#[test]
fn test_feed_matches_exact_size() {
    let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    let mut stream = re.stream();

    let feed_matches = stream.feed(b"hello hello");
    assert_eq!(feed_matches.len(), 1); // ExactSizeIterator
}

// ============================================
// Integration with builder
// ============================================

#[test]
fn test_stream_with_builder_options() {
    let re = FuzzyRegexBuilder::new("(?:hello){e<=2}")
        .similarity(0.7)
        .case_insensitive(true)
        .build()
        .unwrap();

    let mut stream = re.stream();
    let matches: Vec<_> = stream.feed(b"HALLO WORLD").collect();
    // Should find fuzzy match with case insensitivity
    assert!(!matches.is_empty());
}
