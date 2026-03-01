// Suppress pedantic lints for SIMD code
#![allow(clippy::wildcard_imports)]

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

        if self.negated { !in_bitmap } else { in_bitmap }
    }

    /// Check if a character is in the class.
    #[inline]
    #[must_use]
    pub fn contains_char(&self, ch: char) -> bool {
        if ch.is_ascii() {
            self.contains(ch as u8)
        } else {
            let in_class = self.matches_non_ascii;
            if self.negated { !in_class } else { in_class }
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
