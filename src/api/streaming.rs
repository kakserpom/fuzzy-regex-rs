//! Streaming API for fuzzy regex matching.
//!
//! This module provides types for processing text streams incrementally,
//! allowing fuzzy regex matching on large files, network streams, or any
//! byte source without loading everything into memory.
//!
//! # Example
//!
//! ```
//! use fuzzy_regex::FuzzyRegex;
//!
//! let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
//! let mut stream = re.stream();
//!
//! // Feed chunks of data
//! let chunk1 = b"This is a test with hel";
//! let chunk2 = b"lo world in it";
//!
//! for m in stream.feed(chunk1) {
//!     println!("Match at {}-{}", m.start(), m.end());
//! }
//! for m in stream.feed(chunk2) {
//!     println!("Match at {}-{}", m.start(), m.end());
//! }
//!
//! // Finish processing to get any remaining matches
//! if let Some(m) = stream.finish() {
//!     println!("Final match at {}-{}", m.start(), m.end());
//! }
//! ```

use std::io::Read;

use super::FuzzyRegex;
use crate::engine::FuzzyBridge;

/// A match found during streaming search.
///
/// Contains the byte offsets within the entire stream (not just the current chunk).
#[derive(Debug, Clone)]
pub struct StreamingMatch {
    start: usize,
    end: usize,
    edits: u8,
    similarity: f32,
}

impl StreamingMatch {
    /// Create a new streaming match.
    #[inline]
    pub(crate) fn new(start: usize, end: usize, edits: u8, similarity: f32) -> Self {
        Self {
            start,
            end,
            edits,
            similarity,
        }
    }

    /// Get the start byte offset in the stream.
    #[inline]
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Get the end byte offset in the stream.
    #[inline]
    #[must_use]
    pub fn end(&self) -> usize {
        self.end
    }

    /// Get the number of edits (edit distance) for this match.
    #[inline]
    #[must_use]
    pub fn edits(&self) -> u8 {
        self.edits
    }

    /// Get the similarity score (0.0 to 1.0).
    #[inline]
    #[must_use]
    pub fn similarity(&self) -> f32 {
        self.similarity
    }

    /// Get the length of the match in bytes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Check if the match is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// A streaming matcher for incremental fuzzy regex matching.
///
/// This type maintains state across multiple `feed()` calls, allowing
/// matches to span chunk boundaries.
///
/// # Example
///
/// ```
/// use fuzzy_regex::FuzzyRegex;
///
/// let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
/// let mut stream = re.stream();
///
/// // Process data in chunks
/// for chunk in [b"hel".as_slice(), b"lo world".as_slice()] {
///     for m in stream.feed(chunk) {
///         println!("Found match at offset {}", m.start());
///     }
/// }
/// ```
pub struct StreamingMatcher<'r> {
    /// Reference to the compiled regex.
    regex: &'r FuzzyRegex,
    /// Buffer for potential cross-boundary matches.
    buffer: Vec<u8>,
    /// Total bytes processed (global offset).
    global_offset: usize,
    /// Maximum buffer size needed (`pattern_len` + `max_edits`).
    max_buffer_size: usize,
    /// Similarity threshold.
    threshold: f32,
    /// Collected matches from the last feed.
    pending_matches: Vec<StreamingMatch>,
}

impl<'r> StreamingMatcher<'r> {
    /// Create a new streaming matcher.
    pub(crate) fn new(regex: &'r FuzzyRegex, threshold: f32) -> Self {
        // Calculate buffer size based on pattern characteristics
        let max_buffer_size =
            regex.max_pattern_len().unwrap_or(64) + regex.max_edits().unwrap_or(2) as usize + 4; // Extra for UTF-8 boundaries

        Self {
            regex,
            buffer: Vec::with_capacity(max_buffer_size),
            global_offset: 0,
            max_buffer_size,
            threshold,
            pending_matches: Vec::new(),
        }
    }

    /// Feed a chunk of bytes into the matcher.
    ///
    /// Returns an iterator over matches found in this chunk (including
    /// matches that span from the previous chunk).
    pub fn feed(&mut self, chunk: &[u8]) -> FeedMatches<'_> {
        self.pending_matches.clear();

        if chunk.is_empty() {
            return FeedMatches {
                matches: &self.pending_matches,
                index: 0,
            };
        }

        // Combine buffer with new chunk for cross-boundary matching
        let search_data: Vec<u8>;
        let buffer_len = self.buffer.len();
        let search_offset: usize;

        if buffer_len > 0 {
            // Prepend buffer to chunk
            search_data = [&self.buffer[..], chunk].concat();
            search_offset = self.global_offset - buffer_len;
        } else {
            search_data = chunk.to_vec();
            search_offset = self.global_offset;
        }

        // Perform search on combined data
        self.search_bytes(&search_data, search_offset, buffer_len);

        // Update buffer for next chunk - keep last N bytes for cross-boundary matches
        self.buffer.clear();
        let keep_bytes = self.max_buffer_size.min(chunk.len());
        if keep_bytes > 0 {
            let start = chunk.len() - keep_bytes;
            self.buffer.extend_from_slice(&chunk[start..]);
        }

        // Update global offset
        self.global_offset += chunk.len();

        FeedMatches {
            matches: &self.pending_matches,
            index: 0,
        }
    }

    /// Signal end of stream and return any final match.
    ///
    /// Call this after all data has been fed to handle matches at the
    /// end of the stream.
    pub fn finish(&mut self) -> Option<StreamingMatch> {
        if self.buffer.is_empty() {
            return None;
        }

        // Search remaining buffer data
        self.pending_matches.clear();
        let search_offset = self.global_offset - self.buffer.len();
        let buffer_copy = self.buffer.clone();
        self.search_bytes(&buffer_copy, search_offset, 0);

        self.buffer.clear();
        self.pending_matches.pop()
    }

    /// Reset the matcher state for reuse.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.global_offset = 0;
        self.pending_matches.clear();
    }

    /// Get the current position in the stream (total bytes processed).
    #[inline]
    #[must_use]
    pub fn position(&self) -> usize {
        self.global_offset
    }

    /// Search bytes and collect matches.
    fn search_bytes(&mut self, data: &[u8], base_offset: usize, skip_before: usize) {
        // Use the fuzzy bridge for streaming search if available
        if let Some(bridge) = self.regex.fuzzy_bridge() {
            self.search_with_bridge(bridge, data, base_offset, skip_before);
        } else {
            // Fall back to string-based search
            if let Ok(text) = std::str::from_utf8(data) {
                self.search_string_fallback(text, base_offset, skip_before);
            }
        }
    }

    /// Search using the fuzzy bridge (Bitap streaming).
    fn search_with_bridge(
        &mut self,
        bridge: &FuzzyBridge,
        data: &[u8],
        base_offset: usize,
        skip_before: usize,
    ) {
        // Use multi-pattern streaming search
        // Returns (pattern_idx, start, result) where result.end is the actual end
        if let Some((_pattern_idx, start, result)) =
            bridge.find_first_multi_pattern_individual(data, self.threshold, &[0])
        {
            // Include matches that:
            // - End after skip_before (spans into new data), OR
            // - Start at/after skip_before (entirely in new data)
            // Skip matches that end within the buffer (already processed)
            if result.end > skip_before {
                self.pending_matches.push(StreamingMatch::new(
                    base_offset + start,
                    base_offset + result.end,
                    result.total_edits(),
                    result.similarity,
                ));
            }
        }
    }

    /// Fallback search using string API.
    fn search_string_fallback(&mut self, text: &str, base_offset: usize, skip_before: usize) {
        if let Some(m) = self.regex.find(text) {
            // Include matches that end after skip_before (spans into new data)
            if m.end() > skip_before {
                self.pending_matches.push(StreamingMatch::new(
                    base_offset + m.start(),
                    base_offset + m.end(),
                    0,
                    1.0,
                ));
            }
        }
    }

    /// Process a reader, yielding matches.
    ///
    /// Reads the reader in chunks and yields matches as they are found.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use fuzzy_regex::FuzzyRegex;
    /// use std::fs::File;
    /// use std::io::BufReader;
    ///
    /// let re = FuzzyRegex::new("(?:hello){e<=1}").unwrap();
    /// let mut stream = re.stream();
    ///
    /// let file = File::open("large_file.txt").unwrap();
    /// for m in stream.search_reader(BufReader::new(file)) {
    ///     println!("Match at {}-{}", m.start(), m.end());
    /// }
    /// ```
    pub fn search_reader<R: Read>(self, reader: R) -> ReaderMatches<'r, R> {
        ReaderMatches::new(self, reader)
    }
}

/// Iterator over matches from a single `feed()` call.
pub struct FeedMatches<'a> {
    matches: &'a [StreamingMatch],
    index: usize,
}

impl Iterator for FeedMatches<'_> {
    type Item = StreamingMatch;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.matches.len() {
            let m = self.matches[self.index].clone();
            self.index += 1;
            Some(m)
        } else {
            None
        }
    }
}

impl ExactSizeIterator for FeedMatches<'_> {
    fn len(&self) -> usize {
        self.matches.len() - self.index
    }
}

/// Iterator over matches from a reader.
pub struct ReaderMatches<'r, R: Read> {
    matcher: StreamingMatcher<'r>,
    reader: R,
    buffer: Vec<u8>,
    chunk_size: usize,
    pending: Vec<StreamingMatch>,
    pending_index: usize,
    finished: bool,
}

impl<'r, R: Read> ReaderMatches<'r, R> {
    fn new(matcher: StreamingMatcher<'r>, reader: R) -> Self {
        let chunk_size = 8192; // 8KB chunks
        Self {
            matcher,
            reader,
            buffer: vec![0u8; chunk_size],
            chunk_size,
            pending: Vec::new(),
            pending_index: 0,
            finished: false,
        }
    }

    /// Set the chunk size for reading.
    #[must_use]
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self.buffer = vec![0u8; size];
        self
    }
}

impl<R: Read> Iterator for ReaderMatches<'_, R> {
    type Item = StreamingMatch;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Return pending matches first
            if self.pending_index < self.pending.len() {
                let m = self.pending[self.pending_index].clone();
                self.pending_index += 1;
                return Some(m);
            }

            if self.finished {
                return None;
            }

            // Read next chunk
            match self.reader.read(&mut self.buffer) {
                Ok(0) => {
                    // End of stream
                    self.finished = true;
                    if let Some(m) = self.matcher.finish() {
                        return Some(m);
                    }
                    return None;
                }
                Ok(n) => {
                    // Process chunk
                    self.pending.clear();
                    self.pending_index = 0;
                    for m in self.matcher.feed(&self.buffer[..n]) {
                        self.pending.push(m);
                    }
                }
                Err(_) => {
                    self.finished = true;
                    return None;
                }
            }
        }
    }
}

/// Iterator over matches in a byte slice (non-streaming).
pub struct ByteMatches<'r, 't> {
    regex: &'r FuzzyRegex,
    text: &'t [u8],
    last_end: usize,
}

impl<'r, 't> ByteMatches<'r, 't> {
    pub(crate) fn new(regex: &'r FuzzyRegex, text: &'t [u8]) -> Self {
        Self {
            regex,
            text,
            last_end: 0,
        }
    }
}

impl Iterator for ByteMatches<'_, '_> {
    type Item = StreamingMatch;

    fn next(&mut self) -> Option<Self::Item> {
        if self.last_end >= self.text.len() {
            return None;
        }

        // Try to search from last_end
        let search_slice = &self.text[self.last_end..];

        // Use fuzzy bridge if available
        if let Some(bridge) = self.regex.fuzzy_bridge() {
            // Returns (pattern_idx, start, result) where result.end is the actual end
            if let Some((_pattern_idx, start, result)) =
                bridge.find_first_multi_pattern_individual(search_slice, 0.0, &[0])
            {
                let abs_start = self.last_end + start;
                let abs_end = self.last_end + result.end;
                self.last_end = abs_end.max(self.last_end + 1);
                return Some(StreamingMatch::new(
                    abs_start,
                    abs_end,
                    result.total_edits(),
                    result.similarity,
                ));
            }
        } else {
            // Fall back to string API
            if let Ok(text) = std::str::from_utf8(search_slice)
                && let Some(m) = self.regex.find(text)
            {
                let abs_start = self.last_end + m.start();
                let abs_end = self.last_end + m.end();
                self.last_end = abs_end.max(self.last_end + 1);
                return Some(StreamingMatch::new(abs_start, abs_end, 0, 1.0));
            }
        }

        None
    }
}
