//! Prefilter for fast candidate position detection.
//!
//! Uses literal prefixes to quickly skip positions that cannot possibly match,
//! dramatically improving performance for unanchored searches.

#![allow(
    clippy::match_same_arms,
    clippy::too_many_lines,
    clippy::items_after_statements
)]

use memchr::{memchr, memchr2, memchr3, memmem};

/// A prefilter that can quickly find candidate start positions.
#[derive(Debug, Clone)]
pub enum Prefilter {
    /// No prefiltering - try every position.
    None,
    /// Search for a single byte.
    SingleByte {
        /// The byte to search for.
        byte: u8,
        /// Maximum distance from found byte where match could start.
        max_offset: usize,
    },
    /// Search for two possible bytes (for case-insensitive or fuzzy).
    TwoBytes {
        /// First byte to search for.
        byte1: u8,
        /// Second byte to search for.
        byte2: u8,
        /// Maximum distance from found byte where match could start.
        max_offset: usize,
    },
    /// Search for three possible bytes.
    ThreeBytes {
        /// First byte to search for.
        byte1: u8,
        /// Second byte to search for.
        byte2: u8,
        /// Third byte to search for.
        byte3: u8,
        /// Maximum distance from found byte where match could start.
        max_offset: usize,
    },
    /// Search for 4+ possible bytes (uses linear scan).
    MultiBytes {
        /// Collection of bytes to search for.
        bytes: Vec<u8>,
        /// Maximum distance from found byte where match could start.
        max_offset: usize,
    },
    /// Search for an exact literal substring.
    Literal {
        /// The byte sequence to search for.
        needle: Vec<u8>,
    },
    /// Search for a literal prefix with offset (for fuzzy matching).
    LiteralWithOffset {
        /// The byte sequence to search for.
        needle: Vec<u8>,
        /// Maximum distance from found literal where match could start.
        max_offset: usize,
    },
    /// Fast search for exactly 2-byte literal (uses memchr + check).
    /// Much faster than memmem for 2-byte needles.
    TwoByteLiteral {
        /// First byte of the literal.
        byte1: u8,
        /// Second byte of the literal.
        byte2: u8,
        /// Maximum distance from found literal where match could start.
        max_offset: usize,
    },
    /// Search for a literal with fuzzy tolerance.
    FuzzyLiteral {
        /// First byte of the literal (or common variant).
        first_byte: u8,
        /// Alternative first bytes (for substitutions).
        alt_bytes: Vec<u8>,
        /// Maximum edit distance.
        max_edits: usize,
    },
    /// Pigeonhole-based prefilter: for k edits, at least one of (k+1) pieces must match exactly.
    /// Much more selective than single-byte prefiltering for longer patterns.
    PigeonholePieces {
        /// The pattern pieces (at least one must match exactly).
        pieces: Vec<Vec<u8>>,
        /// Offset of each piece within the original pattern.
        offsets: Vec<usize>,
        /// Maximum edit distance.
        max_edits: usize,
    },
}

impl Prefilter {
    /// Create a prefilter for an exact literal.
    #[must_use]
    pub fn exact(literal: &str) -> Self {
        if literal.is_empty() {
            return Prefilter::None;
        }

        let bytes = literal.as_bytes();

        // For ASCII-only short literals, search for first byte
        // This is safe because ASCII characters are single-byte
        if bytes.len() <= 3 && bytes[0] < 128 {
            return Prefilter::SingleByte {
                byte: bytes[0],
                max_offset: 0,
            };
        }

        // For non-ASCII or longer literals, use substring search
        // This ensures we match the full first character, not just its first byte
        Prefilter::Literal {
            needle: bytes.to_vec(),
        }
    }

    /// Create a prefilter for a fuzzy literal.
    ///
    /// Strategy: Even with fuzzy matching, certain bytes from the pattern must
    /// appear in any valid match. We search for bytes from the first positions
    /// that could possibly match.
    #[must_use]
    pub fn fuzzy(literal: &str, max_edits: u8) -> Self {
        if literal.is_empty() {
            return Prefilter::None;
        }

        let bytes = literal.as_bytes();
        let max_edits_usize = max_edits as usize;

        // If edits >= pattern length, any text could match - no useful prefilter
        if max_edits_usize >= bytes.len() {
            return Prefilter::None;
        }

        // For non-ASCII patterns (UTF-8), use substring search on the first character.
        // Single-byte prefilter is ineffective because UTF-8 leading bytes (208, 209 for
        // Cyrillic, 228-233 for CJK) appear in almost every character of that script.
        //
        // Only use this for exact matches (e<=0) or when pattern is long enough that
        // the first character must appear. For fuzzy matches, the first char could be
        // deleted, so we'd need to search for both first and second chars.
        if bytes[0] >= 128 {
            let first_char_len = Self::utf8_char_len(bytes[0]);
            let char_count = bytes.iter().filter(|&&b| (b & 0xC0) != 0x80).count(); // Count UTF-8 start bytes

            // Use substring search only when:
            // 1. First char is multi-byte, AND
            // 2. Either exact match (e<=0), or pattern has enough chars that first must appear
            if first_char_len > 1 && first_char_len <= bytes.len() {
                let first_char_must_appear =
                    max_edits_usize == 0 || char_count > max_edits_usize + 1;

                if first_char_must_appear {
                    // For 2-byte UTF-8 characters (Cyrillic, etc.), use fast TwoByteLiteral
                    // which is much faster than memmem::Finder for short needles
                    if first_char_len == 2 {
                        return Prefilter::TwoByteLiteral {
                            byte1: bytes[0],
                            byte2: bytes[1],
                            max_offset: max_edits_usize,
                        };
                    }
                    // For 3+ byte characters, use memmem
                    let needle = bytes[..first_char_len].to_vec();
                    return Prefilter::LiteralWithOffset {
                        needle,
                        max_offset: max_edits_usize,
                    };
                }
            }
        }

        // For ASCII patterns, collect unique bytes from the first (max_edits + 1) positions.
        let search_depth = (max_edits_usize + 1).min(bytes.len());
        let mut search_bytes: Vec<u8> = Vec::with_capacity(search_depth * 2);

        for &b in bytes.iter().take(search_depth) {
            if !search_bytes.contains(&b) {
                search_bytes.push(b);
                // Also add case variant
                if b.is_ascii_lowercase() {
                    let upper = b.to_ascii_uppercase();
                    if !search_bytes.contains(&upper) {
                        search_bytes.push(upper);
                    }
                } else if b.is_ascii_uppercase() {
                    let lower = b.to_ascii_lowercase();
                    if !search_bytes.contains(&lower) {
                        search_bytes.push(lower);
                    }
                }
            }
        }

        // The max_offset accounts for:
        // - Insertions before the pattern (up to max_edits)
        // - The byte we find might be at position (max_edits) in pattern
        let max_offset = max_edits_usize;

        match search_bytes.len() {
            0 => Prefilter::None,
            1 => Prefilter::SingleByte {
                byte: search_bytes[0],
                max_offset,
            },
            2 => Prefilter::TwoBytes {
                byte1: search_bytes[0],
                byte2: search_bytes[1],
                max_offset,
            },
            3 => Prefilter::ThreeBytes {
                byte1: search_bytes[0],
                byte2: search_bytes[1],
                byte3: search_bytes[2],
                max_offset,
            },
            _ => Prefilter::MultiBytes {
                bytes: search_bytes,
                max_offset,
            },
        }
    }

    /// Get the byte length of a UTF-8 character from its leading byte.
    #[inline]
    fn utf8_char_len(leading_byte: u8) -> usize {
        if leading_byte < 128 {
            1 // ASCII
        } else if leading_byte < 224 {
            2 // 2-byte (Cyrillic, Latin Extended, etc.)
        } else if leading_byte < 240 {
            3 // 3-byte (CJK, etc.)
        } else {
            4 // 4-byte (Emoji, rare scripts)
        }
    }

    /// Create a prefilter for a fuzzy literal with rarity-based byte selection.
    ///
    /// For patterns with small alphabets (like DNA), select the rarest bytes
    /// to minimize false positives. Falls back to streaming if all bytes are common.
    #[must_use]
    pub fn fuzzy_rare(literal: &str, max_edits: u8, text_sample: Option<&[u8]>) -> Self {
        if literal.is_empty() {
            return Prefilter::None;
        }

        let bytes = literal.as_bytes();
        let max_edits_usize = max_edits as usize;

        if max_edits_usize >= bytes.len() {
            return Prefilter::None;
        }

        // If we have a text sample, find the rarest byte in the pattern
        if let Some(sample) = text_sample {
            // Count byte frequencies in sample
            let mut freq = [0usize; 256];
            for &b in sample {
                freq[b as usize] += 1;
            }

            // Find the rarest byte in pattern (within first max_edits+1 positions)
            let search_depth = (max_edits_usize + 1).min(bytes.len());
            let mut rarest_byte = bytes[0];
            let mut rarest_freq = freq[bytes[0] as usize];

            for &b in bytes.iter().take(search_depth) {
                if freq[b as usize] < rarest_freq {
                    rarest_freq = freq[b as usize];
                    rarest_byte = b;
                }
            }

            // If the rarest byte appears in >25% of positions, prefilter won't help much
            if rarest_freq * 4 > sample.len() {
                return Prefilter::None; // Fall back to streaming
            }

            return Prefilter::SingleByte {
                byte: rarest_byte,
                max_offset: max_edits_usize,
            };
        }

        // No sample - use standard fuzzy prefilter
        Self::fuzzy(literal, max_edits)
    }

    /// Create a pigeonhole-based prefilter for fuzzy matching.
    ///
    /// For a pattern of length m with at most k edits, we split the pattern into
    /// (k+1) non-overlapping pieces. By the pigeonhole principle, at least one
    /// piece must match exactly in any valid fuzzy match.
    ///
    /// This is much more selective than single-byte prefiltering for longer patterns.
    /// For example, `"hello"` with k=1 → pieces `["hel", "lo"]`, both 2-3 chars long.
    /// Finding `"hel"` or `"lo"` is much rarer than finding 'h' or 'e'.
    #[must_use]
    pub fn pigeonhole(literal: &str, max_edits: u8) -> Self {
        let bytes = literal.as_bytes();
        let m = bytes.len();
        let k = max_edits as usize;

        // Need at least k+1 bytes to form k+1 pieces of at least 1 byte each
        if m < k + 1 || k == 0 {
            return Self::fuzzy(literal, max_edits);
        }

        // Split pattern into k+1 pieces
        let num_pieces = k + 1;
        let base_piece_len = m / num_pieces;
        let extra = m % num_pieces;

        let mut pieces = Vec::with_capacity(num_pieces);
        let mut offsets = Vec::with_capacity(num_pieces);
        let mut pos = 0;

        for i in 0..num_pieces {
            // Distribute extra bytes among first `extra` pieces
            let piece_len = base_piece_len + usize::from(i < extra);
            offsets.push(pos);
            pieces.push(bytes[pos..pos + piece_len].to_vec());
            pos += piece_len;
        }

        // Only use pigeonhole if pieces are long enough to be selective
        // Single-byte pieces are no better than regular fuzzy prefilter
        let min_piece_len = pieces.iter().map(Vec::len).min().unwrap_or(0);
        if min_piece_len < 2 {
            return Self::fuzzy(literal, max_edits);
        }

        Prefilter::PigeonholePieces {
            pieces,
            offsets,
            max_edits: k,
        }
    }

    /// Create a prefilter for case-insensitive matching.
    #[must_use]
    pub fn case_insensitive(literal: &str) -> Self {
        if literal.is_empty() {
            return Prefilter::None;
        }

        let first = literal.as_bytes()[0];

        if first.is_ascii_alphabetic() {
            Prefilter::TwoBytes {
                byte1: first.to_ascii_lowercase(),
                byte2: first.to_ascii_uppercase(),
                max_offset: 0,
            }
        } else {
            Prefilter::SingleByte {
                byte: first,
                max_offset: 0,
            }
        }
    }

    /// Create a prefilter for multiple fuzzy literals.
    ///
    /// Collects first bytes from all patterns and uses memchr to find candidates.
    /// Each entry is (literal, `max_edits`).
    #[must_use]
    pub fn multi_fuzzy(literals: &[(&str, u8)], case_insensitive: bool) -> Self {
        if literals.is_empty() {
            return Prefilter::None;
        }

        // Collect unique first bytes from all patterns
        let mut search_bytes: Vec<u8> = Vec::new();
        let mut max_offset: usize = 0;

        for (lit, max_edits) in literals {
            if lit.is_empty() {
                continue;
            }

            let bytes = lit.as_bytes();
            let max_edits_usize = *max_edits as usize;

            // If any pattern has edits >= length, can't use prefilter
            if max_edits_usize >= bytes.len() {
                return Prefilter::None;
            }

            // Update max_offset (accounts for insertions at start)
            max_offset = max_offset.max(max_edits_usize);

            // Collect bytes from first (max_edits + 1) positions
            let search_depth = (max_edits_usize + 1).min(bytes.len());
            for &b in bytes.iter().take(search_depth) {
                if !search_bytes.contains(&b) {
                    search_bytes.push(b);
                }
                // Also add case variant
                if case_insensitive && b.is_ascii_alphabetic() {
                    let variant = if b.is_ascii_lowercase() {
                        b.to_ascii_uppercase()
                    } else {
                        b.to_ascii_lowercase()
                    };
                    if !search_bytes.contains(&variant) {
                        search_bytes.push(variant);
                    }
                }
            }
        }

        match search_bytes.len() {
            0 => Prefilter::None,
            1 => Prefilter::SingleByte {
                byte: search_bytes[0],
                max_offset,
            },
            2 => Prefilter::TwoBytes {
                byte1: search_bytes[0],
                byte2: search_bytes[1],
                max_offset,
            },
            3 => Prefilter::ThreeBytes {
                byte1: search_bytes[0],
                byte2: search_bytes[1],
                byte3: search_bytes[2],
                max_offset,
            },
            _ => Prefilter::MultiBytes {
                bytes: search_bytes,
                max_offset,
            },
        }
    }

    /// Find candidate positions in the text.
    /// Returns an iterator over byte positions where a match might start.
    #[must_use]
    pub fn find_candidates<'a>(&'a self, text: &'a [u8]) -> CandidateIter<'a> {
        CandidateIter {
            prefilter: self,
            text,
            pos: 0,
            finder: None,
        }
    }

    /// Check if this prefilter does anything useful.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self, Prefilter::None)
    }

    /// Get the `max_offset` for this prefilter (how far back a match could start).
    #[must_use]
    pub fn max_offset(&self) -> usize {
        match self {
            Prefilter::None | Prefilter::Literal { .. } => 0,
            Prefilter::SingleByte { max_offset, .. }
            | Prefilter::TwoBytes { max_offset, .. }
            | Prefilter::ThreeBytes { max_offset, .. }
            | Prefilter::MultiBytes { max_offset, .. }
            | Prefilter::LiteralWithOffset { max_offset, .. }
            | Prefilter::TwoByteLiteral { max_offset, .. } => *max_offset,
            Prefilter::FuzzyLiteral { max_edits, .. }
            | Prefilter::PigeonholePieces { max_edits, .. } => *max_edits,
        }
    }

    /// Check if this prefilter is "selective" enough to be effective.
    /// A prefilter searching for too many different bytes will hit too many positions.
    /// Returns the number of unique bytes being searched for (lower is better).
    #[must_use]
    pub fn selectivity(&self) -> usize {
        match self {
            Prefilter::None => usize::MAX,
            Prefilter::SingleByte { .. } => 1,
            Prefilter::TwoBytes { .. }
            | Prefilter::ThreeBytes { .. }
            | Prefilter::TwoByteLiteral { .. } => 2,
            Prefilter::MultiBytes { bytes, .. } => bytes.len(),
            Prefilter::Literal { needle } | Prefilter::LiteralWithOffset { needle, .. } => {
                needle.len().min(4)
            }
            Prefilter::FuzzyLiteral { alt_bytes, .. } => 1 + alt_bytes.len(),
            Prefilter::PigeonholePieces { pieces, .. } => {
                pieces.iter().map(Vec::len).min().unwrap_or(1)
            }
        }
    }

    /// Check if this prefilter is likely to be effective for multi-pattern search.
    /// Returns false if the prefilter searches for too many different bytes.
    #[must_use]
    pub fn is_selective(&self) -> bool {
        // More than 6 different bytes starts to become ineffective
        // (each common byte appears ~5-10% of the time in English text)
        // With 6 bytes, we hit ~30-40% of positions which is still reasonable.
        // With 8+ bytes, we hit ~50%+ which makes prefiltering ineffective.
        self.selectivity() <= 6
    }
}

/// Iterator over candidate start positions.
pub struct CandidateIter<'a> {
    /// Reference to the prefilter configuration.
    prefilter: &'a Prefilter,
    /// The text being searched.
    text: &'a [u8],
    /// Current search position in the text.
    pos: usize,
    /// Lazily initialized substring finder for literal searches.
    finder: Option<memmem::Finder<'a>>,
}

impl Iterator for CandidateIter<'_> {
    type Item = usize;

    #[allow(clippy::too_many_lines)]
    fn next(&mut self) -> Option<usize> {
        if self.pos >= self.text.len() {
            return None;
        }

        match self.prefilter {
            Prefilter::None => {
                // Return every position
                let result = self.pos;
                self.pos += 1;
                // Skip to next char boundary
                while self.pos < self.text.len() && !is_char_boundary(self.text, self.pos) {
                    self.pos += 1;
                }
                Some(result)
            }

            Prefilter::SingleByte { byte, max_offset } => {
                if let Some(idx) = memchr(*byte, &self.text[self.pos..]) {
                    let found = self.pos + idx;
                    self.pos = found + 1;

                    // Return position adjusted by max_offset
                    // Match could start up to max_offset before the found byte
                    Some(found.saturating_sub(*max_offset))
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::TwoBytes {
                byte1,
                byte2,
                max_offset,
            } => {
                if let Some(idx) = memchr2(*byte1, *byte2, &self.text[self.pos..]) {
                    let found = self.pos + idx;
                    self.pos = found + 1;
                    Some(found.saturating_sub(*max_offset))
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::ThreeBytes {
                byte1,
                byte2,
                byte3,
                max_offset,
            } => {
                if let Some(idx) = memchr3(*byte1, *byte2, *byte3, &self.text[self.pos..]) {
                    let found = self.pos + idx;
                    self.pos = found + 1;
                    Some(found.saturating_sub(*max_offset))
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::MultiBytes { bytes, max_offset } => {
                // Linear scan for any of the bytes
                let remaining = &self.text[self.pos..];
                if let Some(idx) = remaining.iter().position(|b| bytes.contains(b)) {
                    let found = self.pos + idx;
                    self.pos = found + 1;
                    Some(found.saturating_sub(*max_offset))
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::Literal { needle } => {
                // Initialize finder lazily
                let finder = self
                    .finder
                    .get_or_insert_with(|| memmem::Finder::new(needle));

                if let Some(idx) = finder.find(&self.text[self.pos..]) {
                    let found = self.pos + idx;
                    self.pos = found + 1;
                    Some(found)
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::LiteralWithOffset { needle, max_offset } => {
                // Initialize finder lazily - memmem uses SIMD for fast search
                let finder = self
                    .finder
                    .get_or_insert_with(|| memmem::Finder::new(needle));

                if let Some(idx) = finder.find(&self.text[self.pos..]) {
                    let found = self.pos + idx;
                    self.pos = found + 1;
                    Some(found.saturating_sub(*max_offset))
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::TwoByteLiteral {
                byte1,
                byte2,
                max_offset,
            } => {
                // For short remaining text, use memchr + second byte check (low overhead)
                // For long remaining text, use memmem (SIMD-efficient for many false positives)
                let remaining = &self.text[self.pos..];

                // Threshold tuned empirically: memmem SIMD setup cost ~10-15ns
                // breaks even around 50-100 bytes of scanning
                const MEMMEM_THRESHOLD: usize = 64;

                if remaining.len() < MEMMEM_THRESHOLD {
                    // Short text: use memchr + check (avoid memmem setup)
                    let mut search_pos = 0;
                    while search_pos < remaining.len() {
                        if let Some(idx) = memchr(*byte1, &remaining[search_pos..]) {
                            let abs_idx = search_pos + idx;
                            if abs_idx + 1 < remaining.len() && remaining[abs_idx + 1] == *byte2 {
                                let found = self.pos + abs_idx;
                                self.pos = found + 1;
                                return Some(found.saturating_sub(*max_offset));
                            }
                            search_pos = abs_idx + 1;
                        } else {
                            break;
                        }
                    }
                    self.pos = self.text.len();
                    None
                } else {
                    // Long text: use memmem for SIMD-optimized search
                    let needle = [*byte1, *byte2];
                    let finder = memmem::Finder::new(&needle);
                    if let Some(idx) = finder.find(remaining) {
                        let found = self.pos + idx;
                        self.pos = found + 1;
                        Some(found.saturating_sub(*max_offset))
                    } else {
                        self.pos = self.text.len();
                        None
                    }
                }
            }

            Prefilter::FuzzyLiteral {
                first_byte,
                alt_bytes,
                max_edits,
            } => {
                // Search for first byte or any alternative
                let remaining = &self.text[self.pos..];

                let idx = if alt_bytes.is_empty() {
                    memchr(*first_byte, remaining)
                } else if alt_bytes.len() == 1 {
                    memchr2(*first_byte, alt_bytes[0], remaining)
                } else if alt_bytes.len() == 2 {
                    memchr3(*first_byte, alt_bytes[0], alt_bytes[1], remaining)
                } else {
                    // Fallback to simple search
                    remaining
                        .iter()
                        .position(|&b| b == *first_byte || alt_bytes.contains(&b))
                };

                if let Some(idx) = idx {
                    let found = self.pos + idx;
                    self.pos = found + 1;
                    Some(found.saturating_sub(*max_edits))
                } else {
                    self.pos = self.text.len();
                    None
                }
            }

            Prefilter::PigeonholePieces {
                pieces,
                offsets,
                max_edits,
            } => {
                // Search for any piece match and return adjusted position
                // Strategy: search for first bytes of each piece, verify full match
                let remaining = &self.text[self.pos..];

                // Collect first bytes of all pieces for fast initial scan
                let first_bytes: Vec<u8> = pieces.iter().map(|p| p[0]).collect();

                // Find the next occurrence of any first byte
                let mut search_offset = 0;
                while search_offset < remaining.len() {
                    // Find next position with any of the first bytes
                    let byte_match = match first_bytes.len() {
                        1 => memchr(first_bytes[0], &remaining[search_offset..]),
                        2 => memchr2(first_bytes[0], first_bytes[1], &remaining[search_offset..]),
                        3 => memchr3(
                            first_bytes[0],
                            first_bytes[1],
                            first_bytes[2],
                            &remaining[search_offset..],
                        ),
                        _ => remaining[search_offset..]
                            .iter()
                            .position(|b| first_bytes.contains(b)),
                    };

                    match byte_match {
                        Some(idx) => {
                            let abs_idx = search_offset + idx;
                            let text_pos = self.pos + abs_idx;

                            // Check which piece(s) match at this position
                            for (piece_idx, piece) in pieces.iter().enumerate() {
                                if remaining[abs_idx..].starts_with(piece) {
                                    // Found a piece match!
                                    // The pattern could start at text_pos - offset - max_edits
                                    let piece_offset = offsets[piece_idx];
                                    let candidate_start = text_pos
                                        .saturating_sub(piece_offset)
                                        .saturating_sub(*max_edits);

                                    self.pos = text_pos + 1;
                                    return Some(candidate_start);
                                }
                            }

                            // First byte matched but no piece matched - continue searching
                            search_offset = abs_idx + 1;
                        }
                        None => {
                            // No more first bytes found
                            break;
                        }
                    }
                }

                self.pos = self.text.len();
                None
            }
        }
    }
}

/// Check if a byte position is a UTF-8 char boundary.
#[inline]
fn is_char_boundary(text: &[u8], pos: usize) -> bool {
    if pos >= text.len() {
        return true;
    }
    // UTF-8 continuation bytes start with 10xxxxxx
    (text[pos] & 0b1100_0000) != 0b1000_0000
}

/// Extract a prefilter from pattern information.
#[must_use]
pub fn create_prefilter(
    first_literal: Option<&str>,
    max_edits: Option<u8>,
    case_insensitive: bool,
) -> Prefilter {
    match first_literal {
        None | Some("") => Prefilter::None,
        Some(lit) => {
            if case_insensitive {
                Prefilter::case_insensitive(lit)
            } else if let Some(edits) = max_edits {
                if edits > 0 {
                    Prefilter::fuzzy(lit, edits)
                } else {
                    Prefilter::exact(lit)
                }
            } else {
                Prefilter::exact(lit)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_prefilter() {
        let pf = Prefilter::exact("hello");
        let text = b"say hello world hello";
        let candidates: Vec<_> = pf.find_candidates(text).collect();
        assert_eq!(candidates, vec![4, 16]);
    }

    #[test]
    fn test_single_byte_prefilter() {
        let pf = Prefilter::SingleByte {
            byte: b'h',
            max_offset: 0,
        };
        let text = b"say hello world";
        let candidates: Vec<_> = pf.find_candidates(text).collect();
        assert_eq!(candidates, vec![4]);
    }

    #[test]
    fn test_fuzzy_prefilter() {
        // Fuzzy prefilter searches for bytes from first (max_edits+1) positions
        // "hello" with 2 edits: searches for 'h', 'H', 'e' (first 3 unique bytes with case variants)
        let pf = Prefilter::fuzzy("hello", 2);
        let text = b"say hello world";
        let candidates: Vec<_> = pf.find_candidates(text).collect();
        // 'h' at position 4 → returns 4-2=2
        // 'e' at position 5 → returns 5-2=3
        // 'e' at position 12 → returns 12-2=10
        // 'l' and 'o' are not in search set (only first 3 bytes used)
        assert!(candidates.contains(&2), "Should find 'h' at 4, offset to 2");
        assert!(candidates.contains(&3), "Should find 'e' at 5, offset to 3");

        // For exact match (0 edits), prefilter works normally
        let pf_exact = Prefilter::fuzzy("hello", 0);
        let candidates_exact: Vec<_> = pf_exact.find_candidates(text).collect();
        assert_eq!(candidates_exact, vec![4]); // Just 'h' at position 4

        // For high edit distance >= pattern length, no prefilter
        let pf_high = Prefilter::fuzzy("hi", 2);
        assert!(matches!(pf_high, Prefilter::None));
    }

    #[test]
    fn test_case_insensitive_prefilter() {
        let pf = Prefilter::case_insensitive("Hello");
        let text = b"say HELLO and hello";
        let candidates: Vec<_> = pf.find_candidates(text).collect();
        assert_eq!(candidates, vec![4, 14]); // H at 4, h at 14
    }

    #[test]
    fn test_pigeonhole_prefilter() {
        // "hello" with 1 edit → pieces ["hel", "lo"] (at least one must match exactly)
        let pf = Prefilter::pigeonhole("hello", 1);

        // Should be PigeonholePieces variant
        assert!(matches!(pf, Prefilter::PigeonholePieces { .. }));

        // Text containing "hel" → should find candidate
        let text1 = b"say hel world";
        let candidates1: Vec<_> = pf.find_candidates(text1).collect();
        assert!(!candidates1.is_empty(), "Should find 'hel' piece");

        // Text containing "lo" → should find candidate
        let text2 = b"say lo world";
        let candidates2: Vec<_> = pf.find_candidates(text2).collect();
        assert!(!candidates2.is_empty(), "Should find 'lo' piece");

        // Text containing "hello" → should find candidate at correct position
        let text3 = b"say hello world";
        let candidates3: Vec<_> = pf.find_candidates(text3).collect();
        assert!(!candidates3.is_empty(), "Should find 'hello'");
        // "hel" is at position 4, offset 0 in pattern, max_edits=1 → candidate at 4-0-1=3
        assert!(
            candidates3.contains(&3),
            "Candidate should include position 3"
        );

        // Text without any pieces → no candidates
        let text4 = b"say xyz world";
        let candidates4: Vec<_> = pf.find_candidates(text4).collect();
        assert!(
            candidates4.is_empty(),
            "Should not find candidates in text without pieces"
        );

        // Short patterns fall back to regular fuzzy prefilter
        let pf_short = Prefilter::pigeonhole("hi", 1);
        assert!(!matches!(pf_short, Prefilter::PigeonholePieces { .. }));
    }

    #[test]
    fn test_pigeonhole_pieces_split() {
        // Test piece splitting for various pattern/edit combinations

        // "abcdefgh" (8 chars) with 1 edit → 2 pieces of 4 chars each
        let pf1 = Prefilter::pigeonhole("abcdefgh", 1);
        if let Prefilter::PigeonholePieces {
            pieces, offsets, ..
        } = &pf1
        {
            assert_eq!(pieces.len(), 2);
            assert_eq!(pieces[0], b"abcd");
            assert_eq!(pieces[1], b"efgh");
            assert_eq!(offsets, &[0, 4]);
        } else {
            panic!("Expected PigeonholePieces");
        }

        // "abcdefgh" with 3 edits → 4 pieces of 2 chars each
        let pf2 = Prefilter::pigeonhole("abcdefgh", 3);
        if let Prefilter::PigeonholePieces {
            pieces, offsets, ..
        } = &pf2
        {
            assert_eq!(pieces.len(), 4);
            assert_eq!(pieces[0], b"ab");
            assert_eq!(pieces[1], b"cd");
            assert_eq!(pieces[2], b"ef");
            assert_eq!(pieces[3], b"gh");
            assert_eq!(offsets, &[0, 2, 4, 6]);
        } else {
            panic!("Expected PigeonholePieces");
        }

        // "abcde" (5 chars) with 2 edits → 3 pieces [2, 2, 1] - but min piece < 2, falls back
        let pf3 = Prefilter::pigeonhole("abcde", 2);
        assert!(
            !matches!(pf3, Prefilter::PigeonholePieces { .. }),
            "Should fall back when pieces too short"
        );
    }
}
