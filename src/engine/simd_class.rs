// Suppress pedantic lints for SIMD code
#![allow(clippy::wildcard_imports)]
// Allow unsafe function calls in this module (required for SIMD intrinsics)
#![allow(unsafe_op_in_unsafe_fn)]

//! SIMD-accelerated character class matching.
//!
//! This module provides fast character class membership testing using:
//! 1. 128-bit ASCII bitmap for O(1) single-character lookups
//! 2. SIMD vectorized scanning for finding matches in byte slices

use crate::ir::HirClass;
use crate::parser::ast::{CharClass, CharClassItem, NamedClass};

/// A 128-bit bitmap for fast ASCII character class membership testing.
/// Each bit represents whether the corresponding ASCII byte (0-127) is in the class.
#[derive(Clone, Copy, Debug)]
pub struct AsciiClassBitmap {
    /// Lower 64 bits (bytes 0-63).
    lo: u64,
    /// Upper 64 bits (bytes 64-127).
    hi: u64,
    /// Whether this is a negated class.
    negated: bool,
    /// Whether this class matches non-ASCII characters.
    matches_non_ascii: bool,
}

impl AsciiClassBitmap {
    /// Create an empty bitmap (matches nothing).
    #[must_use]
    pub fn empty() -> Self {
        AsciiClassBitmap {
            lo: 0,
            hi: 0,
            negated: false,
            matches_non_ascii: false,
        }
    }

    /// Create a bitmap that matches all ASCII characters.
    #[must_use]
    pub fn all_ascii() -> Self {
        AsciiClassBitmap {
            lo: u64::MAX,
            hi: u64::MAX,
            negated: false,
            matches_non_ascii: false,
        }
    }

    /// Create a bitmap from an AST `CharClass`.
    #[must_use]
    pub fn from_char_class(class: &CharClass) -> Self {
        let mut bitmap = AsciiClassBitmap::empty();
        bitmap.negated = class.negated;

        for item in &class.items {
            match item {
                CharClassItem::Single(ch) => {
                    if ch.is_ascii() {
                        bitmap.set(*ch as u8);
                    } else {
                        bitmap.matches_non_ascii = true;
                    }
                }
                CharClassItem::Range(start, end) => {
                    let start_byte = if start.is_ascii() { *start as u8 } else { 128 };
                    let end_byte = if end.is_ascii() { *end as u8 } else { 127 };

                    for b in start_byte..=end_byte.min(127) {
                        bitmap.set(b);
                    }
                    // Check if range extends into non-ASCII
                    if *end as u32 > 127 {
                        bitmap.matches_non_ascii = true;
                    }
                }
                CharClassItem::Named(named) => {
                    bitmap.add_named_class(*named);
                }
            }
        }

        bitmap
    }

    /// Create a bitmap from an IR `HirClass`.
    #[must_use]
    pub fn from_hir_class(class: &HirClass) -> Self {
        let mut bitmap = AsciiClassBitmap::empty();

        // For negated character classes with positive named classes (e.g., \D = [^\d]),
        // the named class (Digit) is added as-is to the bitmap, and we rely on
        // bitmap.negated to flip it. This works correctly.
        bitmap.negated = class.negated;

        // Add single characters
        for &ch in &class.chars {
            if ch.is_ascii() {
                bitmap.set(ch as u8);
            } else {
                bitmap.matches_non_ascii = true;
            }
        }

        // Add ranges
        for &(start, end) in &class.ranges {
            let start_byte = if start.is_ascii() { start as u8 } else { 128 };
            let end_byte = if end.is_ascii() { end as u8 } else { 127 };

            for b in start_byte..=end_byte.min(127) {
                bitmap.set(b);
            }
            // Check if range extends into non-ASCII
            if end as u32 > 127 {
                bitmap.matches_non_ascii = true;
            }
        }

        // Add named classes
        for &named in &class.named {
            bitmap.add_named_class(named);
        }

        bitmap
    }

    /// Add a named class to the bitmap.
    fn add_named_class(&mut self, class: NamedClass) {
        match class {
            NamedClass::Digit => {
                for b in b'0'..=b'9' {
                    self.set(b);
                }
            }
            NamedClass::NotDigit => {
                // Set all except digits
                for b in 0u8..=127 {
                    if !b.is_ascii_digit() {
                        self.set(b);
                    }
                }
                self.matches_non_ascii = true;
            }
            NamedClass::Word => {
                for b in b'a'..=b'z' {
                    self.set(b);
                }
                for b in b'A'..=b'Z' {
                    self.set(b);
                }
                for b in b'0'..=b'9' {
                    self.set(b);
                }
                self.set(b'_');
            }
            NamedClass::NotWord => {
                for b in 0u8..=127 {
                    let is_word = b.is_ascii_lowercase()
                        || b.is_ascii_uppercase()
                        || b.is_ascii_digit()
                        || b == b'_';
                    if !is_word {
                        self.set(b);
                    }
                }
                self.matches_non_ascii = true;
            }
            NamedClass::Whitespace => {
                self.set(b' ');
                self.set(b'\t');
                self.set(b'\n');
                self.set(b'\r');
                self.set(0x0C); // form feed
                self.set(0x0B); // vertical tab
            }
            NamedClass::NotWhitespace => {
                for b in 0u8..=127 {
                    if !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0x0B) {
                        self.set(b);
                    }
                }
                self.matches_non_ascii = true;
            }
            NamedClass::Any | NamedClass::AnyExceptNewline => {
                // Set all ASCII
                self.lo = u64::MAX;
                self.hi = u64::MAX;
                if matches!(class, NamedClass::AnyExceptNewline) {
                    self.clear(b'\n');
                    self.clear(b'\r');
                }
                self.matches_non_ascii = true;
            }
        }
    }

    /// Set a bit for the given ASCII byte.
    #[inline]
    fn set(&mut self, byte: u8) {
        if byte < 64 {
            self.lo |= 1u64 << byte;
        } else if byte < 128 {
            self.hi |= 1u64 << (byte - 64);
        }
    }

    /// Clear a bit for the given ASCII byte.
    #[inline]
    fn clear(&mut self, byte: u8) {
        if byte < 64 {
            self.lo &= !(1u64 << byte);
        } else if byte < 128 {
            self.hi &= !(1u64 << (byte - 64));
        }
    }

    /// Check if a byte is in the class.
    #[inline]
    #[must_use]
    pub fn contains(&self, byte: u8) -> bool {
        let in_bitmap = if byte < 64 {
            (self.lo & (1u64 << byte)) != 0
        } else if byte < 128 {
            (self.hi & (1u64 << (byte - 64))) != 0
        } else {
            self.matches_non_ascii
        };

        if self.negated {
            !in_bitmap
        } else {
            in_bitmap
        }
    }

    /// Check if a character is in the class.
    #[inline]
    #[must_use]
    pub fn contains_char(&self, ch: char) -> bool {
        if ch.is_ascii() {
            self.contains(ch as u8)
        } else {
            let in_class = self.matches_non_ascii;
            if self.negated {
                !in_class
            } else {
                in_class
            }
        }
    }

    /// Find the first position in the slice where any byte matches the class.
    /// Returns None if no match is found.
    #[must_use]
    pub fn find_first(&self, haystack: &[u8]) -> Option<usize> {
        // Use SIMD-optimized path for longer slices
        #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
        {
            if haystack.len() >= 16 {
                return self.find_first_simd(haystack);
            }
        }

        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            if haystack.len() >= 16 {
                return self.find_first_simd(haystack);
            }
        }

        // Scalar fallback
        self.find_first_scalar(haystack)
    }

    /// Scalar implementation of `find_first`.
    #[inline]
    fn find_first_scalar(&self, haystack: &[u8]) -> Option<usize> {
        for (i, &byte) in haystack.iter().enumerate() {
            if self.contains(byte) {
                return Some(i);
            }
        }
        None
    }

    /// SIMD implementation for `x86_64` with SSE2.
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    fn find_first_simd(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::x86_64::*;

        // For negated classes or classes matching non-ASCII, fall back to scalar
        // (SIMD path handles simpler cases more efficiently)
        if self.negated || self.matches_non_ascii {
            return self.find_first_scalar(haystack);
        }

        let len = haystack.len();
        let mut i = 0;

        // Process 16 bytes at a time
        unsafe {
            while i + 16 <= len {
                let chunk = _mm_loadu_si128(haystack.as_ptr().add(i).cast::<__m128i>());

                // Check each byte against the bitmap using lookup
                // We use a different strategy: check if any byte is in our set
                // by building a mask of matching positions
                let mut mask = 0u16;

                // Extract bytes and check individually (SSE2 doesn't have good gather)
                let bytes: [u8; 16] = std::mem::transmute(chunk);
                for (j, &b) in bytes.iter().enumerate() {
                    if self.contains(b) {
                        mask |= 1 << j;
                    }
                }

                if mask != 0 {
                    return Some(i + mask.trailing_zeros() as usize);
                }

                i += 16;
            }
        }

        // Handle remaining bytes
        (i..len).find(|&j| self.contains(haystack[j]))
    }

    /// SIMD implementation for aarch64 with NEON.
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    fn find_first_simd(&self, haystack: &[u8]) -> Option<usize> {
        // For negated classes or classes matching non-ASCII, fall back to scalar
        if self.negated || self.matches_non_ascii {
            return self.find_first_scalar(haystack);
        }

        let len = haystack.len();
        let mut i = 0;

        // Process 16 bytes at a time
        unsafe {
            use std::arch::aarch64::*;

            while i + 16 <= len {
                let chunk = vld1q_u8(haystack.as_ptr().add(i));

                // Check each byte against the bitmap
                let bytes: [u8; 16] = std::mem::transmute(chunk);
                for (j, &b) in bytes.iter().enumerate() {
                    if self.contains(b) {
                        return Some(i + j);
                    }
                }

                i += 16;
            }
        }

        // Handle remaining bytes
        (i..len).find(|&j| self.contains(haystack[j]))
    }

    /// Find all positions where bytes match the class.
    /// Returns a vector of indices.
    #[must_use]
    pub fn find_all(&self, haystack: &[u8]) -> Vec<usize> {
        let mut results = Vec::new();
        let mut pos = 0;

        while pos < haystack.len() {
            if let Some(offset) = self.find_first(&haystack[pos..]) {
                results.push(pos + offset);
                pos += offset + 1;
            } else {
                break;
            }
        }

        results
    }

    /// Count how many bytes in the slice match the class.
    #[must_use]
    pub fn count_matches(&self, haystack: &[u8]) -> usize {
        haystack.iter().filter(|&&b| self.contains(b)).count()
    }

    /// Check if the bitmap matches any byte in the slice.
    #[inline]
    #[must_use]
    pub fn matches_any(&self, haystack: &[u8]) -> bool {
        self.find_first(haystack).is_some()
    }
}

impl Default for AsciiClassBitmap {
    fn default() -> Self {
        Self::empty()
    }
}

/// A precompiled character class for fast matching.
/// Combines bitmap for ASCII and handles non-ASCII via the original `CharClass`.
#[derive(Clone, Debug)]
pub struct CompiledCharClass {
    /// Fast ASCII bitmap.
    pub bitmap: AsciiClassBitmap,
    /// Original char class for non-ASCII and complex cases.
    pub original: CharClass,
    /// Unicode mode - enable Unicode character classes.
    pub unicode: bool,
}

impl CompiledCharClass {
    /// Create a compiled character class.
    #[must_use]
    pub fn new(class: &CharClass) -> Self {
        CompiledCharClass {
            bitmap: AsciiClassBitmap::from_char_class(class),
            original: class.clone(),
            unicode: false,
        }
    }

    /// Create a compiled character class with unicode mode.
    #[must_use]
    pub fn new_with_unicode(class: &CharClass, unicode: bool) -> Self {
        CompiledCharClass {
            bitmap: AsciiClassBitmap::from_char_class(class),
            original: class.clone(),
            unicode,
        }
    }

    /// Check if a character matches this class.
    #[inline]
    #[must_use]
    pub fn matches(&self, ch: char) -> bool {
        if ch.is_ascii() {
            self.bitmap.contains(ch as u8)
        } else if self.unicode {
            // In unicode mode, check if non-ASCII chars match via the original class
            // which now has NamedClass with unicode-aware matching
            self.original.matches_unicode(ch)
        } else {
            self.original.matches(ch)
        }
    }

    /// Find the first position where any byte matches.
    #[inline]
    #[must_use]
    pub fn find_first(&self, haystack: &[u8]) -> Option<usize> {
        self.bitmap.find_first(haystack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_bitmap_single() {
        let class = CharClass::new(false, vec![CharClassItem::Single('a')]);
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        assert!(bitmap.contains(b'a'));
        assert!(!bitmap.contains(b'b'));
        assert!(!bitmap.contains(b'A'));
    }

    #[test]
    fn test_ascii_bitmap_range() {
        let class = CharClass::new(false, vec![CharClassItem::Range('a', 'z')]);
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        assert!(bitmap.contains(b'a'));
        assert!(bitmap.contains(b'm'));
        assert!(bitmap.contains(b'z'));
        assert!(!bitmap.contains(b'A'));
        assert!(!bitmap.contains(b'0'));
    }

    #[test]
    fn test_ascii_bitmap_negated() {
        let class = CharClass::new(true, vec![CharClassItem::Range('a', 'z')]);
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        assert!(!bitmap.contains(b'a'));
        assert!(!bitmap.contains(b'z'));
        assert!(bitmap.contains(b'A'));
        assert!(bitmap.contains(b'0'));
        assert!(bitmap.contains(b' '));
    }

    #[test]
    fn test_ascii_bitmap_digit() {
        let class = CharClass::digit();
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        for b in b'0'..=b'9' {
            assert!(bitmap.contains(b), "Should contain digit {}", b as char);
        }
        assert!(!bitmap.contains(b'a'));
        assert!(!bitmap.contains(b' '));
    }

    #[test]
    fn test_ascii_bitmap_word() {
        let class = CharClass::word();
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        assert!(bitmap.contains(b'a'));
        assert!(bitmap.contains(b'Z'));
        assert!(bitmap.contains(b'5'));
        assert!(bitmap.contains(b'_'));
        assert!(!bitmap.contains(b' '));
        assert!(!bitmap.contains(b'-'));
    }

    #[test]
    fn test_find_first() {
        let class = CharClass::new(false, vec![CharClassItem::Range('a', 'z')]);
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        assert_eq!(bitmap.find_first(b"123abc"), Some(3));
        assert_eq!(bitmap.find_first(b"ABC"), None);
        assert_eq!(bitmap.find_first(b"hello"), Some(0));
        assert_eq!(bitmap.find_first(b""), None);
    }

    #[test]
    fn test_find_first_long() {
        let class = CharClass::new(false, vec![CharClassItem::Single('x')]);
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        // Test with text longer than 16 bytes to exercise SIMD path
        let text = b"0123456789abcdefxyz";
        assert_eq!(bitmap.find_first(text), Some(16));

        let text2 = b"01234567890123456789x";
        assert_eq!(bitmap.find_first(text2), Some(20));
    }

    #[test]
    fn test_find_all() {
        let class = CharClass::new(false, vec![CharClassItem::Range('a', 'z')]);
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        let positions = bitmap.find_all(b"a1b2c3");
        assert_eq!(positions, vec![0, 2, 4]);
    }

    #[test]
    fn test_count_matches() {
        let class = CharClass::digit();
        let bitmap = AsciiClassBitmap::from_char_class(&class);

        assert_eq!(bitmap.count_matches(b"abc123def456"), 6);
        assert_eq!(bitmap.count_matches(b"no digits"), 0);
    }

    #[test]
    fn test_compiled_char_class() {
        let class = CharClass::word();
        let compiled = CompiledCharClass::new(&class);

        assert!(compiled.matches('a'));
        assert!(compiled.matches('Z'));
        assert!(compiled.matches('5'));
        assert!(!compiled.matches(' '));
    }
}

/// NEON movemask: extracts the high bit from each byte into a 16-bit mask.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn neon_movemask(v: std::arch::aarch64::uint8x16_t) -> u16 {
    use std::arch::aarch64::*;
    let signs = vreinterpretq_u8_s8(vshrq_n_s8(vreinterpretq_s8_u8(v), 7));
    const MASK_BITS: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
    let mask = vld1_u8(MASK_BITS.as_ptr());
    let lo = vand_u8(vget_low_u8(signs), mask);
    let hi = vand_u8(vget_high_u8(signs), mask);
    (vaddv_u8(lo) as u16) | ((vaddv_u8(hi) as u16) << 8)
}

/// SIMD-accelerated forward/reverse search for character class ranges like `[0-9]`.
///
/// This is optimized for searching through text for characters
/// that fall within specified byte ranges.
#[derive(Clone, Debug)]
pub struct RevSearchRanges {
    ranges: Vec<(u8, u8)>,
}

impl RevSearchRanges {
    /// Create a new RevSearchRanges with the given byte ranges.
    /// Each range is (inclusive_low, inclusive_high).
    #[must_use]
    pub fn new(ranges: Vec<(u8, u8)>) -> Self {
        debug_assert!(!ranges.is_empty() && ranges.len() <= 3);
        Self { ranges }
    }

    /// Find the last position of any byte in any range (reverse search).
    /// Returns None if no match is found.
    #[must_use]
    pub fn find_last(&self, haystack: &[u8]) -> Option<usize> {
        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            if haystack.len() >= 32 {
                return unsafe { self.find_last_avx2(haystack) };
            }
        }
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            if haystack.len() >= 16 {
                return unsafe { self.find_last_neon(haystack) };
            }
        }
        self.find_last_scalar(haystack)
    }

    /// Find the first position of any byte in any range (forward search).
    /// Returns None if no match is found.
    #[must_use]
    pub fn find_first(&self, haystack: &[u8]) -> Option<usize> {
        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            if haystack.len() >= 32 {
                return unsafe { self.find_first_avx2(haystack) };
            }
        }
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            if haystack.len() >= 16 {
                return unsafe { self.find_first_neon(haystack) };
            }
        }
        self.find_first_scalar(haystack)
    }

    /// Scalar fallback for find_last.
    fn find_last_scalar(&self, haystack: &[u8]) -> Option<usize> {
        for i in (0..haystack.len()).rev() {
            if self.matches_byte(haystack[i]) {
                return Some(i);
            }
        }
        None
    }

    /// Scalar fallback for find_first.
    fn find_first_scalar(&self, haystack: &[u8]) -> Option<usize> {
        for i in 0..haystack.len() {
            if self.matches_byte(haystack[i]) {
                return Some(i);
            }
        }
        None
    }

    #[inline]
    pub fn matches_byte(&self, byte: u8) -> bool {
        for &(lo, hi) in &self.ranges {
            if byte >= lo && byte <= hi {
                return true;
            }
        }
        false
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    unsafe fn find_last_avx2(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::x86_64::*;

        let len = haystack.len();
        let n = self.ranges.len();

        let lo0 = _mm256_set1_epi8(self.ranges[0].0 as i8);
        let hi0 = _mm256_set1_epi8(self.ranges[0].1 as i8);

        if len >= 32 {
            let mut pos = len - 32;
            loop {
                let chunk = _mm256_loadu_si256(haystack.as_ptr().add(pos) as *const __m256i);

                let ge0 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo0), chunk);
                let le0 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi0), chunk);
                let mut mask = _mm256_movemask_epi8(_mm256_and_si256(ge0, le0)) as u32;

                if n >= 2 {
                    let lo1 = _mm256_set1_epi8(self.ranges[1].0 as i8);
                    let hi1 = _mm256_set1_epi8(self.ranges[1].1 as i8);
                    let ge1 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo1), chunk);
                    let le1 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi1), chunk);
                    mask |= _mm256_movemask_epi8(_mm256_and_si256(ge1, le1)) as u32;
                }
                if n >= 3 {
                    let lo2 = _mm256_set1_epi8(self.ranges[2].0 as i8);
                    let hi2 = _mm256_set1_epi8(self.ranges[2].1 as i8);
                    let ge2 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo2), chunk);
                    let le2 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi2), chunk);
                    mask |= _mm256_movemask_epi8(_mm256_and_si256(ge2, le2)) as u32;
                }

                if mask != 0 {
                    return Some(pos + 31 - mask.leading_zeros() as usize);
                }

                if pos < 32 {
                    break;
                }
                pos -= 32;
            }
        }

        let gap = if len >= 32 { len % 32 } else { len };
        if gap > 0 {
            let mut buf = [0u8; 32];
            buf[..gap].copy_from_slice(&haystack[..gap]);
            let chunk = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);

            let ge0 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo0), chunk);
            let le0 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi0), chunk);
            let mut mask = _mm256_movemask_epi8(_mm256_and_si256(ge0, le0)) as u32;

            if n >= 2 {
                let lo1 = _mm256_set1_epi8(self.ranges[1].0 as i8);
                let hi1 = _mm256_set1_epi8(self.ranges[1].1 as i8);
                let ge1 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo1), chunk);
                let le1 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi1), chunk);
                mask |= _mm256_movemask_epi8(_mm256_and_si256(ge1, le1)) as u32;
            }
            if n >= 3 {
                let lo2 = _mm256_set1_epi8(self.ranges[2].0 as i8);
                let hi2 = _mm256_set1_epi8(self.ranges[2].1 as i8);
                let ge2 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo2), chunk);
                let le2 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi2), chunk);
                mask |= _mm256_movemask_epi8(_mm256_and_si256(ge2, le2)) as u32;
            }

            mask &= (1u32 << gap) - 1;
            if mask != 0 {
                return Some(31 - mask.leading_zeros() as usize);
            }
        }

        None
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    unsafe fn find_first_avx2(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::x86_64::*;

        let len = haystack.len();
        let n = self.ranges.len();

        let lo0 = _mm256_set1_epi8(self.ranges[0].0 as i8);
        let hi0 = _mm256_set1_epi8(self.ranges[0].1 as i8);

        let mut pos = 0;
        while pos + 32 <= len {
            let chunk = _mm256_loadu_si256(haystack.as_ptr().add(pos) as *const __m256i);

            let ge0 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo0), chunk);
            let le0 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi0), chunk);
            let mut mask = _mm256_movemask_epi8(_mm256_and_si256(ge0, le0)) as u32;

            if n >= 2 {
                let lo1 = _mm256_set1_epi8(self.ranges[1].0 as i8);
                let hi1 = _mm256_set1_epi8(self.ranges[1].1 as i8);
                let ge1 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo1), chunk);
                let le1 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi1), chunk);
                mask |= _mm256_movemask_epi8(_mm256_and_si256(ge1, le1)) as u32;
            }
            if n >= 3 {
                let lo2 = _mm256_set1_epi8(self.ranges[2].0 as i8);
                let hi2 = _mm256_set1_epi8(self.ranges[2].1 as i8);
                let ge2 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo2), chunk);
                let le2 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi2), chunk);
                mask |= _mm256_movemask_epi8(_mm256_and_si256(ge2, le2)) as u32;
            }

            if mask != 0 {
                return Some(pos + mask.trailing_zeros() as usize);
            }
            pos += 32;
        }

        let gap = len % 32;
        if gap > 0 {
            let mut buf = [0u8; 32];
            buf[..gap].copy_from_slice(&haystack[pos..]);
            let chunk = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);

            let ge0 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo0), chunk);
            let le0 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi0), chunk);
            let mut mask = _mm256_movemask_epi8(_mm256_and_si256(ge0, le0)) as u32;

            if n >= 2 {
                let lo1 = _mm256_set1_epi8(self.ranges[1].0 as i8);
                let hi1 = _mm256_set1_epi8(self.ranges[1].1 as i8);
                let ge1 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo1), chunk);
                let le1 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi1), chunk);
                mask |= _mm256_movemask_epi8(_mm256_and_si256(ge1, le1)) as u32;
            }
            if n >= 3 {
                let lo2 = _mm256_set1_epi8(self.ranges[2].0 as i8);
                let hi2 = _mm256_set1_epi8(self.ranges[2].1 as i8);
                let ge2 = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, lo2), chunk);
                let le2 = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, hi2), chunk);
                mask |= _mm256_movemask_epi8(_mm256_and_si256(ge2, le2)) as u32;
            }

            mask &= (1u32 << gap) - 1;
            if mask != 0 {
                return Some(pos + mask.trailing_zeros() as usize);
            }
        }

        None
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe fn find_first_neon(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::aarch64::*;

        let len = haystack.len();
        let ptr = haystack.as_ptr();
        let n = self.ranges.len();
        let lo0 = vdupq_n_u8(self.ranges[0].0);
        let hi0 = vdupq_n_u8(self.ranges[0].1);

        let mut pos = 0;
        while pos + 16 <= len {
            let chunk = vld1q_u8(ptr.add(pos));
            let ge0 = vcgeq_u8(chunk, lo0);
            let le0 = vcleq_u8(chunk, hi0);
            let in0 = vandq_u8(ge0, le0);
            let mut mask = neon_movemask(in0) as u32;

            if n >= 2 {
                let lo1 = vdupq_n_u8(self.ranges[1].0);
                let hi1 = vdupq_n_u8(self.ranges[1].1);
                let ge1 = vcgeq_u8(chunk, lo1);
                let le1 = vcleq_u8(chunk, hi1);
                mask |= neon_movemask(vandq_u8(ge1, le1)) as u32;
            }
            if n >= 3 {
                let lo2 = vdupq_n_u8(self.ranges[2].0);
                let hi2 = vdupq_n_u8(self.ranges[2].1);
                let ge2 = vcgeq_u8(chunk, lo2);
                let le2 = vcleq_u8(chunk, hi2);
                mask |= neon_movemask(vandq_u8(ge2, le2)) as u32;
            }

            if mask != 0 {
                return Some(pos + mask.trailing_zeros() as usize);
            }
            pos += 16;
        }

        let gap = len - pos;
        if gap > 0 {
            let mut buf = [0u8; 16];
            buf[..gap].copy_from_slice(&haystack[pos..]);
            let chunk = vld1q_u8(buf.as_ptr());
            let ge0 = vcgeq_u8(chunk, lo0);
            let le0 = vcleq_u8(chunk, hi0);
            let mut mask = neon_movemask(vandq_u8(ge0, le0)) as u32;

            if n >= 2 {
                let lo1 = vdupq_n_u8(self.ranges[1].0);
                let hi1 = vdupq_n_u8(self.ranges[1].1);
                let ge1 = vcgeq_u8(chunk, lo1);
                let le1 = vcleq_u8(chunk, hi1);
                mask |= neon_movemask(vandq_u8(ge1, le1)) as u32;
            }
            if n >= 3 {
                let lo2 = vdupq_n_u8(self.ranges[2].0);
                let hi2 = vdupq_n_u8(self.ranges[2].1);
                let ge2 = vcgeq_u8(chunk, lo2);
                let le2 = vcleq_u8(chunk, hi2);
                mask |= neon_movemask(vandq_u8(ge2, le2)) as u32;
            }

            mask &= (1u32 << gap) - 1;
            if mask != 0 {
                return Some(pos + mask.trailing_zeros() as usize);
            }
        }

        None
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe fn find_last_neon(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::aarch64::*;

        let len = haystack.len();
        let ptr = haystack.as_ptr();
        let n = self.ranges.len();
        let lo0 = vdupq_n_u8(self.ranges[0].0);
        let hi0 = vdupq_n_u8(self.ranges[0].1);

        if len >= 16 {
            let lo1 = if n >= 2 {
                vdupq_n_u8(self.ranges[1].0)
            } else {
                lo0
            };
            let hi1 = if n >= 2 {
                vdupq_n_u8(self.ranges[1].1)
            } else {
                hi0
            };
            let lo2 = if n >= 3 {
                vdupq_n_u8(self.ranges[2].0)
            } else {
                lo0
            };
            let hi2 = if n >= 3 {
                vdupq_n_u8(self.ranges[2].1)
            } else {
                hi0
            };

            let mut pos = len - 16;
            loop {
                let chunk = vld1q_u8(ptr.add(pos));
                let ge0 = vcgeq_u8(chunk, lo0);
                let le0 = vcleq_u8(chunk, hi0);
                let in0 = vandq_u8(ge0, le0);
                let mut mask = neon_movemask(in0) as u32;

                if n >= 2 {
                    let ge1 = vcgeq_u8(chunk, lo1);
                    let le1 = vcleq_u8(chunk, hi1);
                    mask |= neon_movemask(vandq_u8(ge1, le1)) as u32;
                }
                if n >= 3 {
                    let ge2 = vcgeq_u8(chunk, lo2);
                    let le2 = vcleq_u8(chunk, hi2);
                    mask |= neon_movemask(vandq_u8(ge2, le2)) as u32;
                }

                if mask != 0 {
                    return Some(pos + 15 - mask.leading_zeros() as usize);
                }

                if pos < 16 {
                    break;
                }
                pos -= 16;
            }
        }

        let gap = if len >= 16 { len % 16 } else { len };
        if gap > 0 {
            let mut buf = [0u8; 16];
            buf[..gap].copy_from_slice(&haystack[..gap]);
            let chunk = vld1q_u8(buf.as_ptr());
            let ge0 = vcgeq_u8(chunk, lo0);
            let le0 = vcleq_u8(chunk, hi0);
            let mut mask = neon_movemask(vandq_u8(ge0, le0)) as u32;

            if n >= 2 {
                let lo1 = vdupq_n_u8(self.ranges[1].0);
                let hi1 = vdupq_n_u8(self.ranges[1].1);
                let ge1 = vcgeq_u8(chunk, lo1);
                let le1 = vcleq_u8(chunk, hi1);
                mask |= neon_movemask(vandq_u8(ge1, le1)) as u32;
            }
            if n >= 3 {
                let lo2 = vdupq_n_u8(self.ranges[2].0);
                let hi2 = vdupq_n_u8(self.ranges[2].1);
                let ge2 = vcgeq_u8(chunk, lo2);
                let le2 = vcleq_u8(chunk, hi2);
                mask |= neon_movemask(vandq_u8(ge2, le2)) as u32;
            }

            mask &= (1u32 << gap) - 1;
            if mask != 0 {
                return Some(15 - mask.leading_zeros() as usize);
            }
        }

        None
    }
}

/// Byte frequency table for selecting rare bytes in Teddy-style search.
/// Lower values = rarer = better for SIMD scanning.
static BYTE_FREQ: [u8; 256] = {
    let mut t = [0u8; 256];
    t[0x09] = 70;
    t[0x0A] = 205;
    t[0x0D] = 195;
    t[0x20] = 210;
    t[0x65] = 200;
    t[0x74] = 190;
    t[0x61] = 180;
    t[0x6F] = 175;
    t[0x69] = 170;
    t[0x6E] = 165;
    t[0x73] = 160;
    t[0x68] = 155;
    t[0x72] = 150;
    t[0x64] = 140;
    t[0x6C] = 135;
    t[0x63] = 130;
    t[0x75] = 125;
    t[0x6D] = 120;
    t[0x77] = 115;
    t[0x66] = 110;
    t[0x67] = 105;
    t[0x79] = 100;
    t[0x70] = 95;
    t[0x62] = 90;
    t[0x76] = 85;
    t[0x6B] = 80;
    t[0x6A] = 50;
    t[0x78] = 45;
    t[0x71] = 40;
    t[0x7A] = 35;
    t[0x45] = 30;
    t[0x54] = 29;
    t[0x41] = 28;
    t[0x4F] = 27;
    t[0x49] = 26;
    t[0x4E] = 25;
    t[0x53] = 24;
    t[0x48] = 23;
    t[0x52] = 22;
    t[0x44] = 21;
    t[0x4C] = 20;
    t[0x43] = 19;
    t[0x55] = 18;
    t[0x4D] = 17;
    t[0x57] = 16;
    t[0x46] = 15;
    t[0x47] = 14;
    t[0x59] = 13;
    t[0x50] = 12;
    t[0x42] = 11;
    t[0x56] = 10;
    t[0x4B] = 9;
    t[0x4A] = 8;
    t[0x58] = 7;
    t[0x51] = 6;
    t[0x5A] = 5;
    t[0x30] = 60;
    t[0x31] = 58;
    t[0x32] = 56;
    t[0x33] = 54;
    t[0x34] = 52;
    t[0x35] = 50;
    t[0x36] = 48;
    t[0x37] = 46;
    t[0x38] = 44;
    t[0x39] = 42;
    t[0x2E] = 70;
    t[0x2C] = 65;
    t
};

/// Teddy-style literal search using rare byte selection and SIMD verification.
#[derive(Clone, Debug)]
pub struct TeddySearch {
    needle: Vec<u8>,
    rare_idx: usize,
    rare_byte: u8,
    confirm_idx: usize,
    confirm_byte: u8,
}

impl TeddySearch {
    /// Create a new TeddySearch for the given pattern.
    #[must_use]
    pub fn new(needle: &[u8]) -> Self {
        debug_assert!(!needle.is_empty());

        let mut rare_idx = 0;
        let mut rare_freq = BYTE_FREQ[needle[0] as usize];
        for (i, &b) in needle.iter().enumerate().skip(1) {
            let f = BYTE_FREQ[b as usize];
            if f < rare_freq {
                rare_freq = f;
                rare_idx = i;
            }
        }

        let confirm_idx = if needle.len() > 1 {
            let mut ci = if rare_idx == 0 { 1 } else { 0 };
            let mut cf = BYTE_FREQ[needle[ci] as usize];
            for (i, &b) in needle.iter().enumerate() {
                if i == rare_idx {
                    continue;
                }
                let f = BYTE_FREQ[b as usize];
                if f < cf {
                    cf = f;
                    ci = i;
                }
            }
            ci
        } else {
            0
        };

        Self {
            needle: needle.to_vec(),
            rare_idx,
            rare_byte: needle[rare_idx],
            confirm_idx,
            confirm_byte: needle[confirm_idx],
        }
    }

    /// Pattern length.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.needle.len()
    }

    /// The rare byte used for SIMD scanning.
    #[inline]
    #[must_use]
    pub fn rare_byte(&self) -> u8 {
        self.rare_byte
    }

    #[inline]
    fn verify(&self, haystack: &[u8], start: usize) -> bool {
        let n = self.needle.len();
        if start + n > haystack.len() {
            return false;
        }
        &haystack[start..start + n] == &self.needle
    }

    /// Find the first occurrence of the pattern (forward search).
    #[must_use]
    pub fn find_first(&self, haystack: &[u8]) -> Option<usize> {
        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            if haystack.len() >= 32 && haystack.len() >= self.needle.len() {
                return unsafe { self.find_first_avx2(haystack) };
            }
        }
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            if haystack.len() >= 32 && haystack.len() >= self.needle.len() {
                return unsafe { self.find_first_neon(haystack) };
            }
        }
        self.find_first_scalar(haystack)
    }

    fn find_first_scalar(&self, haystack: &[u8]) -> Option<usize> {
        let n = self.needle.len();
        if haystack.len() < n {
            return None;
        }

        for byte_pos in 0..=haystack.len() - n + self.rare_idx {
            if haystack[byte_pos] == self.rare_byte {
                let candidate = byte_pos.saturating_sub(self.rare_idx);
                if self.verify(haystack, candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    unsafe fn find_first_avx2(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::x86_64::*;

        let nlen = self.needle.len();
        let ptr = haystack.as_ptr();
        let vrare = _mm256_set1_epi8(self.rare_byte as i8);
        let rare_idx = self.rare_idx;
        let confirm_idx = self.confirm_idx;
        let confirm_byte = self.confirm_byte;
        let last_byte_pos = haystack.len() - nlen;

        let mut pos = 0;
        while pos + 32 <= haystack.len() {
            let chunk = _mm256_loadu_si256(ptr.add(pos) as *const __m256i);
            let mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, vrare)) as u32;

            let mut bits = mask;
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                let byte_pos = pos + bit;

                if byte_pos <= last_byte_pos {
                    let candidate = byte_pos.saturating_sub(rare_idx);
                    if candidate + nlen <= haystack.len()
                        && *ptr.add(candidate + confirm_idx) == confirm_byte
                        && self.verify(haystack, candidate)
                    {
                        return Some(candidate);
                    }
                }
                bits &= !(1u32 << bit);
            }
            pos += 32;
        }

        for byte_pos in pos..=last_byte_pos {
            if byte_pos >= rare_idx && byte_pos <= last_byte_pos {
                let candidate = byte_pos - rare_idx;
                if self.verify(haystack, candidate) {
                    return Some(candidate);
                }
            }
        }

        None
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe fn find_first_neon(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::aarch64::*;

        let nlen = self.needle.len();
        let ptr = haystack.as_ptr();
        let vrare = vdupq_n_u8(self.rare_byte);
        let vconfirm = vdupq_n_u8(self.confirm_byte);
        let confirm_offset = self.confirm_idx as isize - self.rare_idx as isize;
        let rare_idx = self.rare_idx;
        let last_byte_pos = haystack.len() - nlen + rare_idx;

        let mut pos = rare_idx;
        while pos + 32 <= haystack.len() {
            let r0 = vceqq_u8(vld1q_u8(ptr.add(pos)), vrare);
            let r1 = vceqq_u8(vld1q_u8(ptr.add(pos + 16)), vrare);
            let c0 = vceqq_u8(
                vld1q_u8(ptr.offset(pos as isize + confirm_offset)),
                vconfirm,
            );
            let c1 = vceqq_u8(
                vld1q_u8(ptr.offset(pos as isize + 16 + confirm_offset)),
                vconfirm,
            );
            let and0 = vandq_u8(r0, c0);
            let and1 = vandq_u8(r1, c1);
            if vmaxvq_u8(vorrq_u8(and0, and1)) == 0 {
                pos += 32;
                continue;
            }
            let mut mask = neon_movemask(and0);
            while mask != 0 {
                let bit = mask.trailing_zeros() as usize;
                let start = pos + bit - rare_idx;
                if start + nlen <= haystack.len() && self.verify(haystack, start) {
                    return Some(start);
                }
                mask &= mask - 1;
            }
            let mut mask = neon_movemask(and1);
            while mask != 0 {
                let bit = mask.trailing_zeros() as usize;
                let start = pos + 16 + bit - rare_idx;
                if start + nlen <= haystack.len() && self.verify(haystack, start) {
                    return Some(start);
                }
                mask &= mask - 1;
            }
            pos += 32;
        }

        while pos <= last_byte_pos {
            if *ptr.add(pos) == self.rare_byte
                && *ptr.offset(pos as isize + confirm_offset) == self.confirm_byte
            {
                let start = pos - rare_idx;
                if self.verify(haystack, start) {
                    return Some(start);
                }
            }
            pos += 1;
        }

        None
    }

    /// Find the last occurrence of the pattern (reverse search).
    #[must_use]
    pub fn find_last(&self, haystack: &[u8]) -> Option<usize> {
        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            if haystack.len() >= 32 && haystack.len() >= self.needle.len() {
                return unsafe { self.find_last_avx2(haystack) };
            }
        }
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            if haystack.len() >= 32 && haystack.len() >= self.needle.len() {
                return unsafe { self.find_last_neon(haystack) };
            }
        }
        self.find_last_scalar(haystack)
    }

    fn find_last_scalar(&self, haystack: &[u8]) -> Option<usize> {
        let n = self.needle.len();
        if haystack.len() < n {
            return None;
        }

        for byte_pos in (0..=haystack.len() - n + self.rare_idx).rev() {
            if haystack[byte_pos] == self.rare_byte {
                let candidate = byte_pos.saturating_sub(self.rare_idx);
                if self.verify(haystack, candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    unsafe fn find_last_avx2(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::x86_64::*;

        let nlen = self.needle.len();
        let ptr = haystack.as_ptr();
        let vrare = _mm256_set1_epi8(self.rare_byte as i8);
        let rare_idx = self.rare_idx;
        let confirm_idx = self.confirm_idx;
        let confirm_byte = self.confirm_byte;

        let last_byte_pos = haystack.len() - nlen + rare_idx;

        let mut pos = haystack.len();
        while pos >= 32 {
            pos -= 32;
            let chunk = _mm256_loadu_si256(ptr.add(pos) as *const __m256i);
            let mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, vrare)) as u32;

            let mut bits = mask;
            while bits != 0 {
                let bit = 31 - bits.leading_zeros() as usize;
                let byte_pos = pos + bit;

                if byte_pos <= last_byte_pos && byte_pos >= rare_idx {
                    let candidate = byte_pos - rare_idx;
                    if *ptr.add(candidate + confirm_idx) == confirm_byte
                        && self.verify(haystack, candidate)
                    {
                        return Some(candidate);
                    }
                }
                bits &= !(1u32 << bit);
            }
        }

        for byte_pos in (0..pos).rev() {
            if byte_pos <= last_byte_pos && byte_pos >= rare_idx {
                let candidate = byte_pos - rare_idx;
                if self.verify(haystack, candidate) {
                    return Some(candidate);
                }
            }
        }

        None
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe fn find_last_neon(&self, haystack: &[u8]) -> Option<usize> {
        use std::arch::aarch64::*;

        let nlen = self.needle.len();
        let ptr = haystack.as_ptr();
        let vrare = vdupq_n_u8(self.rare_byte);
        let vconfirm = vdupq_n_u8(self.confirm_byte);
        let confirm_offset = self.confirm_idx as isize - self.rare_idx as isize;
        let rare_idx = self.rare_idx;

        let last_byte_pos = haystack.len() - nlen + rare_idx;

        let mut pos = haystack.len();
        while pos >= 32 {
            pos -= 32;
            let r0 = vceqq_u8(vld1q_u8(ptr.add(pos)), vrare);
            let r1 = vceqq_u8(vld1q_u8(ptr.add(pos - 16)), vrare);
            let c0 = vceqq_u8(
                vld1q_u8(ptr.offset(pos as isize + confirm_offset)),
                vconfirm,
            );
            let c1 = vceqq_u8(
                vld1q_u8(ptr.offset((pos - 16) as isize + confirm_offset)),
                vconfirm,
            );
            let and0 = vandq_u8(r0, c0);
            let and1 = vandq_u8(r1, c1);
            if vmaxvq_u8(vorrq_u8(and0, and1)) == 0 {
                continue;
            }
            let mut mask = neon_movemask(and0);
            while mask != 0 {
                let bit = 15 - mask.leading_zeros() as usize;
                let byte_pos = pos + bit;
                if byte_pos <= last_byte_pos && byte_pos >= rare_idx {
                    let start = byte_pos - rare_idx;
                    if self.verify(haystack, start) {
                        return Some(start);
                    }
                }
                mask &= !(1u16 << bit);
            }
            let mut mask = neon_movemask(and1);
            while mask != 0 {
                let bit = 15 - mask.leading_zeros() as usize;
                let byte_pos = pos - 16 + bit;
                if byte_pos <= last_byte_pos && byte_pos >= rare_idx {
                    let start = byte_pos - rare_idx;
                    if self.verify(haystack, start) {
                        return Some(start);
                    }
                }
                mask &= !(1u16 << bit);
            }
        }

        for byte_pos in (0..pos).rev() {
            if byte_pos <= last_byte_pos && byte_pos >= rare_idx {
                let start = byte_pos - rare_idx;
                if self.verify(haystack, start) {
                    return Some(start);
                }
            }
        }

        None
    }
}
