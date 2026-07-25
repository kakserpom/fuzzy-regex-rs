//! Bitap algorithm for fast fuzzy string matching.
//!
//! The Bitap algorithm (also known as shift-or or shift-and) uses bitwise
//! operations to perform fuzzy matching very efficiently for short patterns
//! (up to 64 characters).
//!
//! Time complexity: O(n × k) where n = text length, k = max edits
//! Each step involves only a few bitwise operations.

#![allow(
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::inline_always
)]

use super::damlev::{DamLevMatch, EditLimits};
use super::hash::FxHashMap;

/// Fast UTF-8 character decoder - avoids `str::from_utf8` + `chars().next()` overhead.
/// Returns (char, `byte_length`).
///
/// # Safety
/// Assumes input is valid UTF-8. Invalid sequences return replacement char.
#[inline(always)]
fn decode_utf8_char_fast(bytes: &[u8], pos: usize) -> (char, usize) {
    let b0 = bytes[pos];

    if b0 < 128 {
        // ASCII: single byte
        (b0 as char, 1)
    } else if b0 < 224 {
        // 2-byte UTF-8 (Latin Extended, Cyrillic, etc.)
        if pos + 1 < bytes.len() {
            let b1 = bytes[pos + 1];
            let codepoint = ((u32::from(b0) & 0x1F) << 6) | (u32::from(b1) & 0x3F);
            // SAFETY: Valid 2-byte UTF-8 always produces valid codepoint in 0x80-0x7FF range
            (unsafe { char::from_u32_unchecked(codepoint) }, 2)
        } else {
            ('\u{FFFD}', 1)
        }
    } else if b0 < 240 {
        // 3-byte UTF-8 (CJK, etc.)
        if pos + 2 < bytes.len() {
            let b1 = bytes[pos + 1];
            let b2 = bytes[pos + 2];
            let codepoint = ((u32::from(b0) & 0x0F) << 12)
                | ((u32::from(b1) & 0x3F) << 6)
                | (u32::from(b2) & 0x3F);
            // SAFETY: Valid 3-byte UTF-8 produces valid codepoint (excluding surrogates handled by validation)
            (unsafe { char::from_u32_unchecked(codepoint) }, 3)
        } else {
            ('\u{FFFD}', 1)
        }
    } else {
        // 4-byte UTF-8 (Emoji, etc.)
        if pos + 3 < bytes.len() {
            let b1 = bytes[pos + 1];
            let b2 = bytes[pos + 2];
            let b3 = bytes[pos + 3];
            let codepoint = ((u32::from(b0) & 0x07) << 18)
                | ((u32::from(b1) & 0x3F) << 12)
                | ((u32::from(b2) & 0x3F) << 6)
                | (u32::from(b3) & 0x3F);
            // SAFETY: Valid 4-byte UTF-8 always produces valid codepoint
            (unsafe { char::from_u32_unchecked(codepoint) }, 4)
        } else {
            ('\u{FFFD}', 1)
        }
    }
}

/// Word type for Bitap state vectors and masks.  Using u128 allows patterns
/// up to 128 characters; for patterns ≤ 64 the upper bits are all 1s (no
/// match) and the lower 64 bits behave exactly as the previous u64 code.
type Word = u128;

/// Maximum pattern length supported by Bitap (using 128-bit word bitmasks).
pub const MAX_PATTERN_LEN: usize = 128;
/// Maximum pattern length that can use the u64 fast paths (fixed-size
/// arrays, SIMD).  Patterns longer than this use the generic Vec<Word> path.
const FAST_PATH_MAX_LEN: usize = 64;

/// Bitap matcher for fuzzy string matching.
#[derive(Debug)]
pub struct BitapMatcher {
    pattern: String,
    pattern_chars: Vec<char>,
    pattern_len: usize,
    limits: EditLimits,
    case_insensitive: bool,
    /// Character masks: for each character, a bitmask where bit i is 0
    /// if pattern[i] == character.
    char_masks: FxHashMap<char, Word>,
    /// ASCII byte masks for O(1) lookup (all 1s = no match).
    byte_masks: [Word; 128],
    /// Boyer-Moore style skip table: how far to skip when a byte is NOT in pattern.
    /// For bytes in pattern: 0 (can't skip). For bytes not in pattern: `pattern_len` - `max_edits`.
    skip_table: [u8; 256],
    /// Whether the pattern is pure ASCII.
    is_ascii: bool,
    /// Mask with 1 in the position of the last pattern character.
    accept_mask: Word,
    /// Unicode block masks for O(1) lookup of non-ASCII characters.
    /// If all pattern chars are in the same 256-codepoint block, we use this instead of `HashMap`.
    /// `block_base` is the start codepoint (e.g., 0x0400 for Cyrillic).
    unicode_block_base: u32,
    unicode_block_masks: Option<Box<[Word; 256]>>,
    /// When true, a reported match's end is refined to the minimum-edit end
    /// (ties broken by shortest span) instead of the default longest-within-budget
    /// end. Set from `MatchEndPolicy::MinEdit`. Default is false.
    prefer_min_edit: bool,
}

impl BitapMatcher {
    /// Create a new Bitap matcher.
    ///
    /// Returns None if the pattern is too long (> 64 chars).
    pub fn new(pattern: &str, limits: EditLimits, case_insensitive: bool) -> Option<Self> {
        let pattern_chars: Vec<char> = if case_insensitive {
            pattern.to_lowercase().chars().collect()
        } else {
            pattern.chars().collect()
        };

        if pattern_chars.len() > MAX_PATTERN_LEN || pattern_chars.is_empty() {
            return None;
        }

        let pattern_len = pattern_chars.len();
        let is_ascii = pattern_chars.iter().all(char::is_ascii);

        // Build character masks
        // For each character in the alphabet, create a bitmask where bit i is 0
        // if pattern[i] matches the character, 1 otherwise.
        // We use the "shift-or" variant where 0 means match.
        let mut char_masks: FxHashMap<char, Word> = FxHashMap::default();
        let mut byte_masks = [!0u128; 128]; // All 1s = no match

        for (i, &ch) in pattern_chars.iter().enumerate() {
            // Set bit i to 0 for this character (start with all 1s, clear bit i)
            let mask = char_masks.entry(ch).or_insert(!0u128);
            *mask &= !(1u128 << i);

            // Also update byte_masks for ASCII characters
            if ch.is_ascii() {
                let byte = ch as u8;
                byte_masks[byte as usize] &= !(1u128 << i);
                // Handle case insensitivity for ASCII
                if case_insensitive {
                    if byte.is_ascii_lowercase() {
                        byte_masks[byte.to_ascii_uppercase() as usize] &= !(1u128 << i);
                    } else if byte.is_ascii_uppercase() {
                        byte_masks[byte.to_ascii_lowercase() as usize] &= !(1u128 << i);
                    }
                }
            }
        }

        // Accept mask: 1 in position (pattern_len - 1)
        let accept_mask = 1u128 << (pattern_len - 1);

        // Build Boyer-Moore style skip table
        // If a byte is not in the pattern, we can skip ahead when we see it
        let max_edits = limits.max_edits as usize;
        let skip_distance = pattern_len.saturating_sub(max_edits).max(1) as u8;
        let mut skip_table = [skip_distance; 256];

        // Bytes that ARE in the pattern can't be skipped
        for &ch in &pattern_chars {
            if ch.is_ascii() {
                skip_table[ch as usize] = 0;
                // Also mark case variants
                if case_insensitive {
                    if ch.is_ascii_lowercase() {
                        skip_table[ch.to_ascii_uppercase() as usize] = 0;
                    } else if ch.is_ascii_uppercase() {
                        skip_table[ch.to_ascii_lowercase() as usize] = 0;
                    }
                }
            }
        }

        // Build Unicode block lookup table for non-ASCII patterns.
        // If all pattern chars fall within a single 256-codepoint block, we can use O(1) array lookup.
        let (unicode_block_base, unicode_block_masks) =
            Self::build_unicode_block_masks(&pattern_chars, &char_masks);

        Some(BitapMatcher {
            pattern: pattern.to_string(),
            pattern_chars,
            pattern_len,
            limits,
            case_insensitive,
            char_masks,
            byte_masks,
            skip_table,
            is_ascii,
            accept_mask,
            unicode_block_base,
            unicode_block_masks,
            prefer_min_edit: false,
        })
    }

    /// Enable minimum-edit end selection (`MatchEndPolicy::MinEdit`).
    ///
    /// When set, reported matches have their end refined to the minimum-edit
    /// end (fewest edits, then shortest span) rather than the default
    /// longest-within-budget end.
    pub fn set_prefer_min_edit(&mut self, yes: bool) {
        self.prefer_min_edit = yes;
    }

    /// Refine a match's end to the minimum-edit end for its fixed start when
    /// `prefer_min_edit` is set; otherwise return the match unchanged.
    ///
    /// Holding `m.start` fixed, this scans candidate ends across the pattern's
    /// edit window and returns the end that minimizes the edit count (ties
    /// broken by the shortest span). This mirrors `DamLevNfa::find_first`'s
    /// leftmost/fewest-edits/shortest-span selection so that the Bitap-backed
    /// simple-fuzzy fast paths agree with the NFA paths under
    /// `MatchEndPolicy::MinEdit`.
    #[inline]
    fn refine_min_edit_end(&self, m: DamLevMatch, text: &[u8], threshold: f32) -> DamLevMatch {
        if !self.prefer_min_edit {
            return m;
        }
        let max_edits = self.limits.max_edits as usize;
        // Cap the scan window for unlimited/large budgets: a minimal alignment
        // never needs more than `pattern_len` net insertions/deletions.
        let eff_edits = max_edits.min(self.pattern_len.max(1));
        let min_len = self.pattern_len.saturating_sub(eff_edits).max(1);
        let max_len = self.pattern_len + eff_edits;

        let start = m.start;
        let mut chosen: Option<DamLevMatch> = None;
        let mut end = start;
        let mut char_count = 0usize;
        while end < text.len() && char_count < max_len {
            let (_, char_len) = decode_utf8_char_fast(text, end);
            end += char_len;
            char_count += 1;
            if char_count < min_len {
                continue;
            }
            let (insertions, deletions, substitutions, swaps) =
                self.compute_exact_edit_breakdown(&text[start..end]);
            let total = insertions + deletions + substitutions + swaps;
            if total as usize > max_edits {
                continue;
            }
            let sim = self.calc_similarity(total, insertions, deletions);
            if sim < threshold {
                continue;
            }
            let cand = DamLevMatch {
                start,
                end,
                insertions,
                deletions,
                substitutions,
                swaps,
                similarity: sim,
            };
            chosen = Some(match chosen {
                None => cand,
                Some(cur) => {
                    // Prefer fewest edits, then the span closest to the pattern
                    // length (natural alignment), then the shortest span. The
                    // length tie-break keeps a full-length alignment (e.g.
                    // "regex" for `error{e}`) over a degenerate short one (e.g.
                    // "r" via 4 deletions), matching mrab-regex's reporting.
                    let key = |m: &DamLevMatch| {
                        let span = m.end - m.start;
                        (m.total_edits(), span.abs_diff(self.pattern_len), span)
                    };
                    if key(&cand) < key(&cur) { cand } else { cur }
                }
            });
        }
        chosen.unwrap_or(m)
    }

    /// Returns the original pattern string.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Returns the pattern as a slice of characters.
    #[must_use]
    pub fn pattern_chars(&self) -> &[char] {
        &self.pattern_chars
    }

    /// Build Unicode block lookup table for O(1) character mask access.
    /// Returns (`block_base`, `Some(masks)`) if all non-ASCII chars are in a single 256-codepoint block.
    fn build_unicode_block_masks(
        pattern_chars: &[char],
        char_masks: &FxHashMap<char, Word>,
    ) -> (u32, Option<Box<[Word; 256]>>) {
        // Find non-ASCII characters
        let non_ascii: Vec<char> = pattern_chars
            .iter()
            .filter(|c| !c.is_ascii())
            .copied()
            .collect();

        if non_ascii.is_empty() {
            return (0, None);
        }

        // Check if all non-ASCII chars are in the same 256-codepoint block
        let first_cp = non_ascii[0] as u32;
        let block_base = first_cp & !0xFF; // Round down to block start (e.g., 0x0400 for Cyrillic)

        let all_in_block = non_ascii.iter().all(|&ch| {
            let cp = ch as u32;
            (cp & !0xFF) == block_base
        });

        if !all_in_block {
            return (0, None);
        }

        // Build the lookup table
        let mut masks = Box::new([!0u128; 256]);
        for (&ch, &mask) in char_masks {
            let cp = ch as u32;
            if (cp & !0xFF) == block_base {
                let idx = (cp & 0xFF) as usize;
                masks[idx] = mask;
            }
        }

        (block_base, Some(masks))
    }

    /// Get character mask for a character (all 1s if not in pattern).
    #[inline(always)]
    fn get_mask(&self, ch: char) -> Word {
        let cp = ch as u32;

        // Fast path: check Unicode block lookup table
        if let Some(ref masks) = self.unicode_block_masks
            && (cp & !0xFF) == self.unicode_block_base
        {
            return masks[(cp & 0xFF) as usize];
        }

        // Fallback to HashMap
        *self.char_masks.get(&ch).unwrap_or(&!0u128)
    }

    /// Get mask directly from 2-byte UTF-8 sequence (avoids char decode).
    /// Returns (mask, 2) if successful, or falls back to `decode_utf8_char_fast`.
    #[inline(always)]
    fn get_mask_2byte(&self, b0: u8, b1: u8) -> Word {
        if let Some(ref masks) = self.unicode_block_masks {
            // Compute codepoint index directly from UTF-8 bytes
            // For 2-byte UTF-8: codepoint = ((b0 & 0x1F) << 6) | (b1 & 0x3F)
            // Block check: (codepoint & !0xFF) == block_base
            // Index: codepoint & 0xFF
            let codepoint_low6 = (u32::from(b0) & 0x1F) << 6;
            let codepoint = codepoint_low6 | (u32::from(b1) & 0x3F);
            if (codepoint & !0xFF) == self.unicode_block_base {
                return masks[(codepoint & 0xFF) as usize];
            }
        }
        // Fallback: decode to char and lookup
        let codepoint = ((u32::from(b0) & 0x1F) << 6) | (u32::from(b1) & 0x3F);
        let ch = unsafe { char::from_u32_unchecked(codepoint) };
        *self.char_masks.get(&ch).unwrap_or(&!0u128)
    }

    /// Get Boyer-Moore skip distance for a byte.
    /// Returns 0 if byte is in pattern, otherwise returns skip distance.
    #[inline(always)]
    #[must_use]
    pub fn get_skip(&self, byte: u8) -> usize {
        self.skip_table[byte as usize] as usize
    }

    /// Find the next position worth checking using Boyer-Moore skipping.
    /// Scans from `start` looking for a byte that's in the pattern.
    /// Returns the position of the first pattern-relevant byte, or `text.len()` if none.
    #[inline]
    #[must_use]
    pub fn find_next_candidate(&self, text: &[u8], start: usize) -> usize {
        let mut pos = start;
        while pos < text.len() {
            let skip = self.skip_table[text[pos] as usize];
            if skip == 0 {
                return pos;
            }
            pos += skip as usize;
        }
        text.len()
    }

    /// Calculate similarity score.
    fn calc_similarity(&self, edits: u8, insertions: u8, deletions: u8) -> f32 {
        let pattern_len = self.pattern_len as f32;
        if pattern_len == 0.0 {
            return 1.0;
        }

        let edit_distance = f32::from(edits);
        let matched_len = pattern_len + f32::from(insertions) - f32::from(deletions);
        let max_len = pattern_len.max(matched_len).max(1.0);

        (1.0 - edit_distance / max_len).max(0.0)
    }

    /// Myers' bit-vector algorithm for fast O(n) edit distance computation.
    /// Returns the edit distance between pattern and text.
    ///
    /// This is much faster than full DP (O(n) vs O(m×n)) but doesn't give
    /// breakdown of edit types. Use for fast verification.
    #[inline]
    fn compute_edit_distance_myers(&self, text_chars: &[char]) -> u8 {
        let m = self.pattern_len;
        let n = text_chars.len();

        if m == 0 {
            return n as u8;
        }
        if n == 0 {
            return m as u8;
        }

        // Myers' algorithm using our precomputed masks
        // Note: Our masks have bit=0 for match, which is the inverse of typical Myers
        // We adapt by inverting the eq mask
        let mut pv = !0u128; // positive vertical delta (all 1s)
        let mut mv = 0u128; // negative vertical delta (all 0s)
        let mut score = m as u8;

        let mask = 1u128 << (m - 1);

        for &text_char in text_chars {
            // Get pattern equality mask (bit i is 1 if pattern[i] matches text_char)
            // Our stored masks have 0 for match, so we invert
            let eq = if text_char.is_ascii() {
                !self.byte_masks[text_char as usize]
            } else {
                !self.get_mask(text_char)
            };

            let xv = eq | mv;
            let xh = ((eq & pv).wrapping_add(pv)) ^ pv | eq;

            let ph = mv | !(xh | pv);
            let mh = pv & xh;

            // Update score
            if (ph & mask) != 0 {
                score = score.saturating_add(1);
            }
            if (mh & mask) != 0 {
                score = score.saturating_sub(1);
            }

            // Shift for next column
            let ph_shift = (ph << 1) | 1;
            let mh_shift = mh << 1;

            pv = mh_shift | !(xv | ph_shift);
            mv = ph_shift & xv;
        }

        score
    }

    /// Fast edit breakdown using Myers for distance + length heuristic for breakdown.
    /// Returns (insertions, deletions, substitutions, swaps).
    ///
    /// This approximates the breakdown based on:
    /// - Total distance from Myers (exact)
    /// - Length difference (insertions - deletions = `text_len` - `pattern_len`)
    ///
    /// Transpositions are counted as substitutions in this fast version.
    #[inline]
    #[allow(dead_code)]
    fn compute_edit_breakdown_fast(&self, text_chars: &[char]) -> (u8, u8, u8, u8) {
        let m = self.pattern_len;
        let n = text_chars.len();

        if m == 0 {
            return (n as u8, 0, 0, 0);
        }
        if n == 0 {
            return (0, m as u8, 0, 0);
        }

        let distance = self.compute_edit_distance_myers(text_chars);

        // Heuristic: length difference determines insertions vs deletions
        let len_diff = n as i32 - m as i32;

        if len_diff >= 0 {
            // Text is longer or equal: extra chars are insertions
            let insertions = (len_diff as u8).min(distance);
            let other_edits = distance.saturating_sub(insertions);
            (insertions, 0, other_edits, 0)
        } else {
            // Text is shorter: missing chars are deletions
            let deletions = ((-len_diff) as u8).min(distance);
            let other_edits = distance.saturating_sub(deletions);
            (0, deletions, other_edits, 0)
        }
    }

    /// Find all matches in text using Bitap algorithm with k errors.
    #[must_use]
    pub fn find_all(&self, text: &str, threshold: f32) -> Vec<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        // Cap all loops at pattern_len: matches with more edits than the
        // pattern has characters have ≤ 50% similarity and are never useful.
        // This turns O(n·max_edits) into O(n·pattern_len) for unbounded `{e}`.
        let effective_max = max_edits.min(self.pattern_len);
        let text_chars: Vec<(usize, char)> = text.char_indices().collect();

        if text_chars.is_empty() {
            return vec![];
        }

        let mut matches: FxHashMap<(usize, usize), DamLevMatch> = FxHashMap::default();

        // State vectors: R[d] tracks matching state with exactly d errors
        // Bit i is 0 if we've matched pattern[0..=i] with d errors
        // Use two buffers and swap to avoid allocation per character
        let mut r: Vec<u128> = vec![!0u128; effective_max + 1];
        let mut old_r: Vec<u128> = vec![!0u128; effective_max + 1];

        // Initialize: we can delete up to k characters from the start of pattern
        // R[d] starts with first d bits as 0 (matched d chars via deletion)
        // Left shift advances pattern position (bit i → bit i+1)
        for d in 1..=effective_max {
            r[d] = r[d - 1] << 1;
        }

        for (char_idx, &(_, text_char)) in text_chars.iter().enumerate() {
            let text_char = if self.case_insensitive {
                text_char.to_lowercase().next().unwrap_or(text_char)
            } else {
                text_char
            };

            let char_mask = self.get_mask(text_char);

            // Swap buffers: old_r gets previous r, r will be updated (no allocation!)
            std::mem::swap(&mut r, &mut old_r);

            // Update R[0] (exact matching) - use old_r since we swapped
            r[0] = (old_r[0] << 1) | char_mask;

            // Update R[d] for d > 0 (fuzzy matching)
            for d in 1..=effective_max {
                // Can insert from R[d-1]: consume text char without advancing pattern
                let insert = old_r[d - 1];

                // Can delete from R[d-1]: advance pattern without consuming text
                // Uses r[d-1] (already updated) with << 1 to advance pattern position
                let delete = r[d - 1] << 1;

                // Can substitute from R[d-1]: consume both and treat as match
                let substitute = old_r[d - 1] << 1;

                // Regular match with d errors
                let match_d = (old_r[d] << 1) | char_mask;

                r[d] = match_d & insert & delete & substitute;
            }

            // Check for matches (bit pattern_len-1 is 0)
            let end_byte = text_chars.get(char_idx + 1).map_or(text.len(), |(b, _)| *b);

            // Match-checking loop is capped at effective_max (= max_edits.min(pattern_len)).
            for d in 0..=effective_max {
                if (r[d] & self.accept_mask) == 0 {
                    // Pre-filter: skip error levels where the best possible
                    // similarity is below threshold.
                    let sim_max = 1.0f32 - d as f32 / (self.pattern_len + d) as f32;
                    if sim_max < threshold {
                        continue;
                    }
                    // Found a match with d edits
                    // Estimate start position (approximate)
                    let min_start_char = char_idx.saturating_sub(self.pattern_len + d);
                    let max_start_char =
                        char_idx.saturating_sub(self.pattern_len.saturating_sub(d + 1));

                    for start_char in min_start_char..=max_start_char.min(char_idx) {
                        let start_byte = text_chars.get(start_char).map_or(0, |(b, _)| *b);

                        // Fast early-reject: compute bit-parallel Levenshtein
                        // distance. If it exceeds d, the full Damerau DP
                        // (which can only be ≤ Levenshtein) would also exceed
                        // d, so skip the expensive DP entirely.
                        let text_slice = &text.as_bytes()[start_byte..end_byte];
                        if self.quick_reject(text_slice, d) {
                            continue;
                        }

                        // Compute exact edit breakdown using DP
                        let (insertions, deletions, substitutions, swaps) = self
                            .compute_exact_edit_breakdown(text_slice);

                        // Use actual edit count from DP, verify it matches Bitap state
                        let total_edits = insertions + deletions + substitutions + swaps;
                        if total_edits as usize > d {
                            continue; // More edits than this state allows
                        }
                        let sim = self.calc_similarity(total_edits, insertions, deletions);
                        if sim >= threshold {
                            let key = (start_byte, end_byte);
                            let m = DamLevMatch {
                                start: start_byte,
                                end: end_byte,
                                insertions,
                                deletions,
                                substitutions,
                                swaps,
                                similarity: sim,
                            };

                            matches
                                .entry(key)
                                .and_modify(|existing| {
                                    if m.similarity > existing.similarity {
                                        *existing = m.clone();
                                    }
                                })
                                .or_insert(m);
                        }
                    }
                }
            }
        }

        // Handle text shorter than pattern: positions reached during the last iteration
        // need extra propagation (via deletion) to reach the accept position.
        if text_chars.len() < self.pattern_len {
            let chars_short = self.pattern_len - text_chars.len();
            for _ in 0..chars_short.min(effective_max) {
                std::mem::swap(&mut r, &mut old_r);

                // Apply deletion propagation: advance pattern position without consuming text.
                // From d-1 errors at position p, we can delete pattern[p] to reach d errors at p+1.
                r[0] = old_r[0]; // Can't advance without consuming text or adding error
                for d in 1..=effective_max {
                    // Deletion: skip pattern char without consuming text
                    let delete = old_r[d - 1] << 1;
                    // Keep existing state if already matched
                    r[d] = old_r[d] & delete;
                }

                // Check for matches after propagation
                for d in 0..=effective_max {
                    if (r[d] & self.accept_mask) == 0 {
                        // Pre-filter: skip error levels where the best possible
                        // similarity is below threshold.
                        let sim_max = 1.0f32 - d as f32 / (self.pattern_len + d) as f32;
                        if sim_max < threshold {
                            continue;
                        }
                        let end_byte = text.len();
                        let min_start_char = text_chars
                            .len()
                            .saturating_sub(self.pattern_len.saturating_sub(d + 1));

                        for start_char in 0..=min_start_char.min(text_chars.len().saturating_sub(1))
                        {
                            let start_byte = text_chars.get(start_char).map_or(0, |(b, _)| *b);

                            let text_slice = &text.as_bytes()[start_byte..end_byte];

                            // Fast early-reject via Levenshtein-only Bitap
                            if self.quick_reject(text_slice, max_edits) {
                                continue;
                            }

                            let (insertions, deletions, substitutions, swaps) = self
                                .compute_exact_edit_breakdown(text_slice);

                            let total_edits = insertions + deletions + substitutions + swaps;
                            if total_edits as usize <= max_edits {
                                let sim = self.calc_similarity(total_edits, insertions, deletions);
                                if sim >= threshold {
                                    let key = (start_byte, end_byte);
                                    let m = DamLevMatch {
                                        start: start_byte,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    };

                                    matches
                                        .entry(key)
                                        .and_modify(|existing| {
                                            if m.similarity > existing.similarity {
                                                *existing = m.clone();
                                            }
                                        })
                                        .or_insert(m);
                                }
                            }
                        }
                    }
                }
            }
        }

        matches.into_values().collect()
    }

    /// Find all non-overlapping matches, preferring best (highest similarity) matches.
    ///
    /// This method finds all overlapping candidates, sorts by similarity, then
    /// greedily selects non-overlapping matches starting from highest similarity.
    /// This ensures we prefer "Lorem" (sim=1.0) over "ore" (sim=0.6).
    ///
    /// Matches must be at least `pattern_len - max_edits` characters long to be
    /// considered valid. This prevents overly short fuzzy matches.
    ///
    /// When `require_first_char` is true, matches must start with the same first
    /// character as the pattern (case-insensitive). This filters out spurious
    /// matches like "bore" when searching for "Lorem".
    #[must_use]
    pub fn find_best_non_overlapping(
        &self,
        text: &str,
        threshold: f32,
        require_first_char: bool,
    ) -> Vec<DamLevMatch> {
        // Get all overlapping matches
        let mut all_matches = self.find_all(text, threshold);

        if all_matches.is_empty() {
            return vec![];
        }

        // Filter: minimum match length = pattern_len - max_edits
        let min_match_len = self
            .pattern_len
            .saturating_sub(self.limits.max_edits as usize);
        all_matches.retain(|m| m.end - m.start >= min_match_len);

        // Filter: require first character to match pattern's first char
        // Respects case_insensitive setting - if case-sensitive, require exact first char match
        if require_first_char && !self.pattern_chars.is_empty() {
            let pattern_first = self.pattern_chars[0];
            let text_bytes = text.as_bytes();
            all_matches.retain(|m| {
                if m.start >= text_bytes.len() {
                    return false;
                }
                // Decode the first character of the match
                let (first_char, _) = decode_utf8_char_fast(text_bytes, m.start);
                if self.case_insensitive {
                    first_char.eq_ignore_ascii_case(&pattern_first)
                } else {
                    first_char == pattern_first
                }
            });
        }

        if all_matches.is_empty() {
            return vec![];
        }

        // Sort by similarity descending, then by start position ascending
        all_matches.sort_by(|a, b| match b.similarity.partial_cmp(&a.similarity) {
            Some(std::cmp::Ordering::Equal) | None => a.start.cmp(&b.start),
            Some(ord) => ord,
        });

        // Greedily select non-overlapping matches
        let mut result = Vec::new();
        let mut occupied = vec![false; text.len() + 1];

        for m in all_matches {
            // Check if this match overlaps with any already selected
            let overlaps = (m.start..m.end).any(|i| occupied[i]);
            if !overlaps {
                // Mark this range as occupied
                for i in m.start..m.end {
                    occupied[i] = true;
                }
                result.push(m);
            }
        }

        // Sort result by start position for consistent ordering
        result.sort_by_key(|m| m.start);
        result
    }

    /// Fast find of non-overlapping matches optimized for iteration (greedy leftmost).
    ///
    /// This is faster than `find_all()` followed by filtering because:
    /// 1. It skips ahead after each match (no overlapping work)
    /// 2. It only verifies the most likely start position per match
    /// 3. For exact matches (d=0), it trusts Bitap without DP
    ///
    /// When `require_first_char` is true, matches must start with the same first
    /// character as the pattern (case-sensitive unless `case_insensitive` mode).
    ///
    /// Note: This uses greedy-leftmost strategy. For best-match selection
    /// (preferring higher similarity), use `find_best_non_overlapping` instead.
    #[must_use]
    pub fn find_all_non_overlapping(
        &self,
        text: &str,
        threshold: f32,
        require_first_char: bool,
    ) -> Vec<DamLevMatch> {
        self.find_non_overlapping_impl(text, threshold, require_first_char, 0)
    }

    /// Find the first match using the same algorithm as `find_all_non_overlapping`.
    /// Returns as soon as a match is found, avoiding scanning the rest of the text.
    #[must_use]
    pub fn find_first_non_overlapping(&self, text: &str, threshold: f32) -> Option<DamLevMatch> {
        // Try ASCII fast path if both pattern and text are ASCII
        // (only for patterns that fit in u64; longer patterns use the u128 path)
        if self.is_ascii && text.is_ascii() && self.pattern_len <= FAST_PATH_MAX_LEN {
            if let Some(m) = self.find_first_ascii_fast(text.as_bytes(), threshold) {
                return Some(m);
            }
            // If fast path returns None due to max_edits > 4, fall through to generic path
            if self.limits.max_edits <= 4 {
                return None;
            }
        }

        let matches = self.find_non_overlapping_impl(text, threshold, false, 1);
        matches.into_iter().next()
    }

    /// Find up to `n` non-overlapping matches.
    /// Stops searching after finding `n` matches for efficiency.
    #[must_use]
    pub fn find_n_non_overlapping(
        &self,
        text: &str,
        threshold: f32,
        require_first_char: bool,
        n: usize,
    ) -> Vec<DamLevMatch> {
        self.find_non_overlapping_impl(text, threshold, require_first_char, n)
    }

    /// Implementation of non-overlapping match search with optional limit.
    /// `limit` of 0 means unlimited matches, otherwise stops after finding `limit` matches.
    fn find_non_overlapping_impl(
        &self,
        text: &str,
        threshold: f32,
        require_first_char: bool,
        limit: usize,
    ) -> Vec<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        // Cap all loops at pattern_len: matches with more edits than the
        // pattern has characters have near-zero similarity and are never
        // useful.  This turns O(n·max_edits) into O(n·pattern_len) for
        // unbounded `{e}` (max_edits=255).
        let effective_max = max_edits.min(self.pattern_len);
        let text_bytes = text.as_bytes();
        let text_len = text_bytes.len();

        // Handle empty text: matches if pattern can be fully deleted
        if text_len == 0 {
            if self.pattern_len <= max_edits {
                let deletions = self.pattern_len as u8;
                let sim = self.calc_similarity(deletions, 0, deletions);
                if sim >= threshold {
                    return vec![DamLevMatch {
                        start: 0,
                        end: 0,
                        insertions: 0,
                        deletions,
                        substitutions: 0,
                        swaps: 0,
                        similarity: sim,
                    }];
                }
            }
            return vec![];
        }

        // Precompute first-char check if needed
        let first_char_check: Option<char> = if require_first_char && !self.pattern_chars.is_empty()
        {
            Some(self.pattern_chars[0])
        } else {
            None
        };

        let mut matches = Vec::new();
        let mut last_end = 0usize;

        // State vectors (3 buffers for transposition support)
        let mut r: Vec<u128> = vec![!0u128; effective_max + 1];
        let mut old_r: Vec<u128> = vec![!0u128; effective_max + 1];
        let mut old_old_r: Vec<u128> = vec![!0u128; effective_max + 1];

        // Initialize deletion states
        for d in 1..=effective_max {
            r[d] = r[d - 1] << 1;
        }

        // Track pending fuzzy match (wait for potential better exact match)
        let mut pending_match: Option<(usize, DamLevMatch)> = None; // (edit_level, match)
        let mut chars_since_pending = 0usize;

        // Track previous character mask for transposition
        let mut prev_mask: u128 = !0;

        // Circular buffer to track byte positions of recent characters
        // Used to correctly compute start position for matches with multi-byte UTF-8
        let history_size = self.pattern_len + max_edits + 1;
        let mut byte_history: Vec<usize> = vec![0; history_size];
        let mut history_idx = 0usize;

        let mut byte_pos = 0;
        let mut char_count = 0usize;

        while byte_pos < text_len {
            // Decode current character
            let (text_char, char_len) = decode_utf8_char_fast(text_bytes, byte_pos);
            let text_char = if self.case_insensitive {
                text_char.to_lowercase().next().unwrap_or(text_char)
            } else {
                text_char
            };

            // Record byte position in circular buffer for correct UTF-8 start computation
            byte_history[history_idx] = byte_pos;
            history_idx = (history_idx + 1) % history_size;

            let char_mask = self.get_mask(text_char);

            // Rotate buffers: old_old_r <- old_r <- r
            std::mem::swap(&mut old_old_r, &mut old_r);
            std::mem::swap(&mut old_r, &mut r);

            // Update R[0] (exact matching)
            r[0] = (old_r[0] << 1) | char_mask;

            // Update R[d] for d > 0 (fuzzy matching with transposition)
            for d in 1..=effective_max {
                let insert = old_r[d - 1];
                let delete = r[d - 1] << 1;
                let substitute = old_r[d - 1] << 1;
                let match_d = (old_r[d] << 1) | char_mask;
                let mut new_r = match_d & insert & delete & substitute;

                // Transposition: check if we can swap adjacent chars
                // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
                let trans_valid_mask = char_mask | (prev_mask >> 1);
                // From matched position k, we can reach k+2 via transposition at k+1
                let trans = ((old_old_r[d - 1] << 1) | trans_valid_mask) << 1;
                new_r &= trans;

                r[d] = new_r;
            }

            // Update prev_mask for next iteration
            prev_mask = char_mask;

            let end_byte = byte_pos + char_len;

            // Check for match at each error level (prefer lower error levels)
            // Optimization: when we already have a pending fuzzy match, only
            // run the expensive DP verification for error levels that could
            // improve on the pending match (d < pending_d).  Higher d levels
            // are skipped (just the cheap bitap AND check), dramatically
            // reducing per-char cost during the commit wait period.
            let pending_d = pending_match.as_ref().map(|(d, _)| *d);
            'error_levels: for d in 0..=effective_max {
                if (r[d] & self.accept_mask) == 0 {
                    // Pre-filter: skip error levels where even the best possible
                    // match (all substitutions, matched_len = pattern_len) has
                    // similarity below threshold.  This avoids expensive DP
                    // calls for high-d false positives that are common with
                    // unbounded `{e}` (max_edits=255).
                    let sim_max = 1.0f32 - d as f32 / (self.pattern_len + d) as f32;
                    if sim_max < threshold {
                        continue;
                    }
                    // Skip expensive DP if we already have a pending match
                    // at a LOWER d level — it's strictly better.  Matches at
                    // the same d level might have a longer/better span, so
                    // we still check those.
                    if pending_d.is_some_and(|pd| pd < d) {
                        continue;
                    }

                    // Found a potential match with d edits
                    // For fuzzy matches, the match length could vary:
                    // - With deletions: match is shorter than pattern
                    // - With insertions: match is longer than pattern
                    let min_match_len = self.pattern_len.saturating_sub(d);
                    let max_match_len = (self.pattern_len + d).min(char_count + 1);

                    // Track best candidate at this error level
                    let mut best_at_level: Option<DamLevMatch> = None;

                    // Try match lengths from longest to shortest.
                    // The longest valid match = earliest start = leftmost match,
                    // which is our preference.  Breaking on first valid DP
                    // reduces O(pattern_len+d) DP calls to O(1) in the common
                    // case where the longest candidate passes.
                    for try_len in (min_match_len..=max_match_len).rev() {
                        // Compute start_byte by going back try_len characters (not bytes)
                        // Use the circular buffer to handle multi-byte UTF-8 correctly
                        let start_byte = if try_len <= history_size && try_len > 0 {
                            // Look up the byte position from the circular buffer
                            // history_idx points to the next slot, so most recent is (history_idx - 1)
                            // We need to go back (try_len - 1) more slots from there
                            let idx = (history_idx + history_size - try_len) % history_size;
                            byte_history[idx]
                        } else if try_len == 0 {
                            end_byte
                        } else {
                            // try_len > history_size: shouldn't happen normally, fall back to byte math
                            // This could be inaccurate for multi-byte chars
                            end_byte.saturating_sub(try_len)
                        };

                        if start_byte >= end_byte {
                            continue;
                        }

                        // Skip empty matches at end of text (can happen when try_len=0)
                        if start_byte >= text_len {
                            continue;
                        }

                        // Skip if this match overlaps with previous confirmed match
                        if start_byte < last_end {
                            continue;
                        }

                        // Check first-char filter if required
                        if let Some(pattern_first) = first_char_check {
                            let (match_first, _) = decode_utf8_char_fast(text_bytes, start_byte);
                            let matches_first = if self.case_insensitive {
                                match_first.eq_ignore_ascii_case(&pattern_first)
                            } else {
                                match_first == pattern_first
                            };
                            if !matches_first {
                                continue;
                            }
                        }

                        // For exact match (d=0), accept immediately
                        if d == 0 {
                            let sim = 1.0f32;
                            if sim >= threshold {
                                // Clear any pending fuzzy match (this exact match is better)
                                pending_match = None;
                                chars_since_pending = 0;

                                matches.push(DamLevMatch {
                                    start: start_byte,
                                    end: end_byte,
                                    insertions: 0,
                                    deletions: 0,
                                    substitutions: 0,
                                    swaps: 0,
                                    similarity: sim,
                                });

                                // Early exit: return immediately if limit reached
                                if limit > 0 && matches.len() >= limit {
                                    return matches;
                                }

                                last_end = end_byte;

                                // Reset state for next non-overlapping match
                                r.fill(!0u128);
                                old_r.fill(!0u128);
                                old_old_r.fill(!0u128);
                                for dd in 1..=effective_max {
                                    r[dd] = r[dd - 1] << 1;
                                }
                                prev_mask = !0;
                                break 'error_levels; // Found exact match, move on
                            }
                        } else {
                            // For fuzzy match, verify with DP
                            let matched_text = &text_bytes[start_byte..end_byte];
                            // Fast early-reject via Levenshtein-only Bitap
                            if self.quick_reject(matched_text, max_edits) {
                                continue;
                            }
                            let (insertions, deletions, substitutions, swaps) =
                                self.compute_exact_edit_breakdown(matched_text);

                            let total_edits = insertions + deletions + substitutions + swaps;
                            if total_edits as usize <= max_edits {
                                let sim = self.calc_similarity(total_edits, insertions, deletions);
                                if sim >= threshold {
                                    let candidate = DamLevMatch {
                                        start: start_byte,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    };

                                    // Since we try longest first, the first
                                    // valid match IS the earliest start.
                                    best_at_level = Some(candidate);
                                    break; // Found best match at this level
                                }
                            }
                        }
                    }

                    // If we found a valid match at this error level, update pending
                    if let Some(candidate) = best_at_level {
                        // Check if this is better than existing pending match
                        let dominated = pending_match.as_ref().is_some_and(|(pd, pm)| {
                            let pm_len = pm.end - pm.start;
                            let cand_len = candidate.end - candidate.start;
                            *pd < d
                                || (*pd == d && pm.start < candidate.start)
                                || (*pd == d && pm.start == candidate.start && pm_len >= cand_len)
                        });

                        if !dominated {
                            // Only skip the counter reset for very long
                            // patterns at max error level, where
                            // low-complexity text causes every text length
                            // to give te == d, growing the pending match
                            // indefinitely.  For all other cases, always
                            // reset (original behavior) to avoid premature
                            // commits that miss better matches at lower d.
                            let should_reset = if d >= effective_max && self.pattern_len > 32 {
                                let cand_te = candidate.insertions as usize
                                    + candidate.deletions as usize
                                    + candidate.substitutions as usize
                                    + candidate.swaps as usize;
                                match &pending_match {
                                    None => true,
                                    Some((pd, pm)) if *pd == d && pm.start == candidate.start => {
                                        let pm_te = pm.insertions as usize
                                            + pm.deletions as usize
                                            + pm.substitutions as usize
                                            + pm.swaps as usize;
                                        cand_te != pm_te
                                    }
                                    Some(_) => true,
                                }
                            } else {
                                true
                            };
                            if should_reset {
                                chars_since_pending = 0;
                            }
                            pending_match = Some((d, candidate));
                        }
                        break 'error_levels; // Found valid fuzzy match at this level
                    }
                }
            }

            // Check if we should commit the pending fuzzy match
            if let Some((d, ref m)) = pending_match {
                chars_since_pending += 1;
                let match_len = m.end - m.start;

                // Determine when to commit:
                // - Exact matches (d=0): commit immediately
                // - Fuzzy matches (d>0): wait at least 1 char to let potential exact matches appear
                //   This handles cases like "mhussei" (fuzzy at d=2) vs "hussein" (exact at d=0)
                //   where the exact match ends one character later
                let commit_threshold = if d == 0 {
                    1 // Exact match: commit on first check
                } else if d >= effective_max && self.pattern_len > 32 {
                    // Very long pattern at max error level: commit quickly.
                    // Low-complexity text (e.g. all-identical chars) with a
                    // long pattern triggers an explosion of DP calls because
                    // every text length gives te == d, growing the pending
                    // match indefinitely.  The should_reset logic above plus
                    // this short threshold caps the scan at ~2 chars.
                    2
                } else if match_len >= self.pattern_len {
                    2 // Full-length fuzzy match: wait 1 char for potential exact match
                } else {
                    // Short match: wait for potential exact/better match to appear.
                    // Use min(max_edits, 2*pattern_len)+1 to cap the wait for
                    // unbounded `{e}` (max_edits=255) while preserving the original
                    // behavior for bounded edits (e.g. `~1` → commit after 2 chars).
                    // The d_limit optimization makes the per-char wait cost just
                    // the cheap bitap AND check, so a longer wait is affordable.
                    max_edits.min(2 * self.pattern_len) + 1
                };

                if chars_since_pending >= commit_threshold {
                    let (_, m) = pending_match.take().unwrap();
                    let m = self.refine_min_edit_end(m, text_bytes, threshold);
                    last_end = m.end;
                    matches.push(m);

                    // Early exit: return immediately if limit reached
                    if limit > 0 && matches.len() >= limit {
                        return matches;
                    }

                    // Reset state
                    r.fill(!0u128);
                    old_r.fill(!0u128);
                    old_old_r.fill(!0u128);
                    for dd in 1..=effective_max {
                        r[dd] = r[dd - 1] << 1;
                    }
                    prev_mask = !0;
                    chars_since_pending = 0;
                }
            }

            byte_pos = end_byte;
            char_count += 1;
        }

        // Commit any remaining pending match
        if let Some((_, m)) = pending_match {
            let m = self.refine_min_edit_end(m, text_bytes, threshold);
            matches.push(m);
        }

        matches
    }

    /// Ultra-fast ASCII-only path for finding first match.
    /// Only called when both pattern and text are pure ASCII.
    /// Uses stack arrays instead of Vec, direct byte lookup instead of `HashMap`.
    ///
    /// Returns Some((start, end, edits)) on match, None if no match found.
    #[inline]
    fn find_first_ascii_fast(&self, text: &[u8], threshold: f32) -> Option<DamLevMatch> {
        debug_assert!(self.is_ascii);

        let max_edits = self.limits.max_edits as usize;
        let effective_max = max_edits.min(self.pattern_len);
        let text_len = text.len();

        // Handle empty text
        if text_len == 0 {
            if self.pattern_len <= max_edits {
                let deletions = self.pattern_len as u8;
                let sim = self.calc_similarity(deletions, 0, deletions);
                if sim >= threshold {
                    return Some(DamLevMatch {
                        start: 0,
                        end: 0,
                        insertions: 0,
                        deletions,
                        substitutions: 0,
                        swaps: 0,
                        similarity: sim,
                    });
                }
            }
            return None;
        }

        // Use fixed-size arrays for state vectors (up to 4 edits supported in fast path)
        // For more edits, fall back to Vec-based implementation
        if max_edits > 4 {
            return None; // Signal caller to use generic path
        }

        // State vectors (3 buffers for transposition support) - stack allocated
        let mut r: [u128; 5] = [!0u128; 5];
        let mut old_r: [u128; 5] = [!0u128; 5];
        let mut old_old_r: [u128; 5] = [!0u128; 5];

        // Initialize deletion states
        for d in 1..=effective_max {
            r[d] = r[d - 1] << 1;
        }

        // Track pending fuzzy match
        let mut pending_match: Option<(usize, DamLevMatch)> = None;
        let mut chars_since_pending = 0usize;

        // Track previous character mask for transposition
        let mut prev_mask: u128 = !0;

        // Circular buffer for start position tracking (pattern_len + max_edits + 1)
        // Since ASCII: 1 byte = 1 char, we can use byte positions directly
        let history_size = self.pattern_len + max_edits + 1;
        // Use fixed array - max pattern is 64, max edits is 4, so max history is 69
        let mut byte_history: [usize; 72] = [0; 72];
        let mut history_idx = 0usize;

        let mut byte_pos = 0;

        // Pre-fetch for case insensitivity - use a static lookup table
        let to_lower: fn(u8) -> u8 = if self.case_insensitive {
            |b| b.to_ascii_lowercase()
        } else {
            |b| b
        };

        while byte_pos < text_len {
            let byte = to_lower(text[byte_pos]);

            // Record byte position
            byte_history[history_idx % history_size] = byte_pos;
            history_idx += 1;

            // Direct byte mask lookup - no HashMap, no UTF-8 decode
            let char_mask = if byte < 128 {
                self.byte_masks[byte as usize]
            } else {
                !0u128 // Non-ASCII byte: no match (shouldn't happen in ASCII path)
            };

            // Rotate buffers
            let tmp = old_old_r;
            old_old_r = old_r;
            old_r = r;
            r = tmp;

            // Update R[0] (exact matching)
            r[0] = (old_r[0] << 1) | char_mask;

            // Update R[d] for d > 0 (fuzzy matching with transposition)
            for d in 1..=effective_max {
                let insert = old_r[d - 1];
                let delete = r[d - 1] << 1;
                let substitute = old_r[d - 1] << 1;
                let match_d = (old_r[d] << 1) | char_mask;
                let mut new_r = match_d & insert & delete & substitute;

                // Transposition
                let trans_valid_mask = char_mask | (prev_mask >> 1);
                let trans = ((old_old_r[d - 1] << 1) | trans_valid_mask) << 1;
                new_r &= trans;

                r[d] = new_r;
            }

            prev_mask = char_mask;
            let end_byte = byte_pos + 1; // ASCII: 1 byte per char

            // Check for match at each error level
            'error_levels: for d in 0..=effective_max {
                if (r[d] & self.accept_mask) == 0 {
                    let min_match_len = self.pattern_len.saturating_sub(d);
                    let max_match_len = self.pattern_len + d;

                    let mut best_at_level: Option<DamLevMatch> = None;

                    for try_len in min_match_len..=max_match_len {
                        let start_byte =
                            if try_len <= history_size && try_len > 0 && history_idx >= try_len {
                                byte_history[(history_idx - try_len) % history_size]
                            } else if try_len == 0 {
                                end_byte
                            } else {
                                end_byte.saturating_sub(try_len)
                            };

                        if start_byte >= end_byte || start_byte >= text_len {
                            continue;
                        }

                        // For exact match (d=0), return immediately
                        if d == 0 {
                            let sim = 1.0f32;
                            if sim >= threshold {
                                return Some(DamLevMatch {
                                    start: start_byte,
                                    end: end_byte,
                                    insertions: 0,
                                    deletions: 0,
                                    substitutions: 0,
                                    swaps: 0,
                                    similarity: sim,
                                });
                            }
                        } else {
                            // Fuzzy match - verify with DP
                            let matched_text = &text[start_byte..end_byte];
                            // Fast early-reject via Levenshtein-only Bitap
                            if self.quick_reject(matched_text, max_edits) {
                                continue;
                            }
                            let (insertions, deletions, substitutions, swaps) =
                                self.compute_exact_edit_breakdown(matched_text);

                            let total_edits = insertions + deletions + substitutions + swaps;
                            if total_edits as usize <= max_edits {
                                let sim = self.calc_similarity(total_edits, insertions, deletions);
                                if sim >= threshold {
                                    let candidate = DamLevMatch {
                                        start: start_byte,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    };

                                    let dominated = best_at_level.as_ref().is_some_and(|best| {
                                        let best_len = best.end - best.start;
                                        let cand_len = candidate.end - candidate.start;
                                        best.start < candidate.start
                                            || (best.start == candidate.start
                                                && best_len >= cand_len)
                                    });

                                    if !dominated {
                                        best_at_level = Some(candidate);
                                    }
                                }
                            }
                        }
                    }

                    if let Some(candidate) = best_at_level {
                        let dominated = pending_match.as_ref().is_some_and(|(pd, pm)| {
                            let pm_len = pm.end - pm.start;
                            let cand_len = candidate.end - candidate.start;
                            *pd < d
                                || (*pd == d && pm.start < candidate.start)
                                || (*pd == d && pm.start == candidate.start && pm_len >= cand_len)
                        });

                        if !dominated {
                            // Only skip the counter reset for very long
                            // patterns at max error level, where
                            // low-complexity text causes every text length
                            // to give te == d, growing the pending match
                            // indefinitely.  For all other cases, always
                            // reset (original behavior) to avoid premature
                            // commits that miss better matches at lower d.
                            let should_reset = if d >= effective_max && self.pattern_len > 32 {
                                let cand_te = candidate.insertions as usize
                                    + candidate.deletions as usize
                                    + candidate.substitutions as usize
                                    + candidate.swaps as usize;
                                match &pending_match {
                                    None => true,
                                    Some((pd, pm)) if *pd == d && pm.start == candidate.start => {
                                        let pm_te = pm.insertions as usize
                                            + pm.deletions as usize
                                            + pm.substitutions as usize
                                            + pm.swaps as usize;
                                        cand_te != pm_te
                                    }
                                    Some(_) => true,
                                }
                            } else {
                                true
                            };
                            if should_reset {
                                chars_since_pending = 0;
                            }
                            pending_match = Some((d, candidate));
                        }
                        break 'error_levels;
                    }
                }
            }

            // Check if we should commit pending match
            if let Some((d, ref m)) = pending_match {
                chars_since_pending += 1;
                let match_len = m.end - m.start;

                let commit_threshold = if d == 0 {
                    1
                } else if d >= effective_max && self.pattern_len > 32 {
                    2
                } else if match_len >= self.pattern_len {
                    2
                } else {
                    max_edits + 1
                };

                if chars_since_pending >= commit_threshold {
                    let (_, m) = pending_match.take().unwrap();
                    return Some(self.refine_min_edit_end(m, text, threshold));
                }
            }

            byte_pos += 1;
        }

        // Return any pending match
        pending_match.map(|(_, m)| self.refine_min_edit_end(m, text, threshold))
    }

    /// Total Damerau-Levenshtein distance between the pattern and `matched_text`.
    ///
    /// This is a lightweight (single `u8` per cell) gate used before the full
    /// per-operation breakdown: match similarity depends only on the *total*
    /// distance and the window length (`calc_similarity` reduces to
    /// `1 - dist / max(m, L)`), so most candidate windows can be accepted or
    /// rejected here without the heavier 5-tuple DP. Returns `None` for windows
    /// too large for the stack buffers (the caller then falls back to the full
    /// breakdown, which handles the heap case).
    fn compute_damerau_distance(&self, matched_text: &[u8]) -> Option<u8> {
        // Matches the breakdown DP's stack limit; larger windows return None and
        // the caller falls back to the full (heap-capable) breakdown.
        const N: usize = 72;

        let pattern = &self.pattern_chars;
        let m = pattern.len();
        if m >= N {
            return None;
        }
        if matched_text.is_empty() {
            return Some(m.min(255) as u8);
        }
        if m == 0 {
            return Some(
                matched_text
                    .iter()
                    .filter(|&&b| b & 0xC0 != 0x80)
                    .count()
                    .min(255) as u8,
            );
        }

        // ASCII fast path: DP directly over the bytes with no char buffer (the
        // buffer init is the dominant per-call cost for the short windows this
        // gate runs on). `is_ascii` means every pattern char is < 0x80.
        if self.is_ascii && matched_text.is_ascii() {
            let n = matched_text.len();
            if n >= N {
                return None;
            }
            let lc = |b: u8| {
                if self.case_insensitive {
                    b.to_ascii_lowercase()
                } else {
                    b
                }
            };
            let mut prev_prev = [0u8; N + 1];
            let mut prev = [0u8; N + 1];
            let mut curr = [0u8; N + 1];
            for (j, slot) in prev.iter_mut().enumerate().take(n + 1) {
                *slot = j as u8;
            }
            let mut prev_pb = 0u8;
            for i in 1..=m {
                let pb = lc(pattern[i - 1] as u8);
                curr[0] = i as u8;
                for j in 1..=n {
                    let tb = lc(matched_text[j - 1]);
                    if pb == tb {
                        curr[j] = prev[j - 1];
                    } else {
                        let mut best = 1 + prev[j - 1].min(curr[j - 1]).min(prev[j]);
                        if i > 1 && j > 1 && pb == lc(matched_text[j - 2]) && prev_pb == tb {
                            best = best.min(prev_prev[j - 2] + 1);
                        }
                        curr[j] = best;
                    }
                }
                std::mem::swap(&mut prev_prev, &mut prev);
                std::mem::swap(&mut prev, &mut curr);
                prev_pb = pb;
            }
            return Some(prev[n]);
        }

        // Non-ASCII: decode to a char buffer and run the same distance-only DP.
        // The breakdown DP now yields the exact Damerau distance for every window
        // (see `compute_exact_edit_breakdown`), so this gate matches it here too
        // (verified by `test_breakdown_is_valid_and_exact`).
        let text_str = std::str::from_utf8(matched_text).ok()?;
        let mut text_chars_buf: [char; N] = ['\0'; N];
        let mut n = 0;
        for c in text_str.chars() {
            if n >= N {
                return None;
            }
            text_chars_buf[n] = if self.case_insensitive {
                c.to_ascii_lowercase()
            } else {
                c
            };
            n += 1;
        }
        let text_chars = &text_chars_buf[..n];

        let mut prev_prev = [0u8; N + 1];
        let mut prev = [0u8; N + 1];
        let mut curr = [0u8; N + 1];
        for (j, slot) in prev.iter_mut().enumerate().take(n + 1) {
            *slot = j as u8;
        }
        let mut prev_pattern_char = '\0';
        for i in 1..=m {
            let pattern_char = if self.case_insensitive {
                pattern[i - 1].to_ascii_lowercase()
            } else {
                pattern[i - 1]
            };
            curr[0] = i as u8;
            for j in 1..=n {
                let text_char = text_chars[j - 1];
                if pattern_char == text_char {
                    curr[j] = prev[j - 1];
                } else {
                    let mut best = 1 + prev[j - 1].min(curr[j - 1]).min(prev[j]);
                    if i > 1
                        && j > 1
                        && pattern_char == text_chars[j - 2]
                        && prev_pattern_char == text_char
                    {
                        best = best.min(prev_prev[j - 2] + 1);
                    }
                    curr[j] = best;
                }
            }
            std::mem::swap(&mut prev_prev, &mut prev);
            std::mem::swap(&mut prev, &mut curr);
            prev_pattern_char = pattern_char;
        }

        Some(prev[n])
    }

    /// Fast early-reject using Damerau-Levenshtein Bitap (with transposition).
    ///
    /// Runs the same Bitap state machine as the main algorithm but with only
    /// `budget + 1` error levels (instead of `effective_max + 1`).
    /// Uses the existing `byte_masks` for O(1) lookups.
    ///
    /// Returns `true` if the edit distance definitely exceeds `budget`
    /// (pattern cannot match this text slice with ≤ budget errors).
    /// Returns `false` if a match is possible — the caller should verify
    /// with the full Damerau-Levenshtein DP.
    #[inline]
    fn quick_reject(&self, text: &[u8], budget: usize) -> bool {
        let m = self.pattern_len;
        if m == 0 || budget >= m {
            return false;
        }

        // Budget 0: exact match check (very fast)
        // For case-sensitive: direct byte comparison.
        // For case-insensitive: fall through to Bitap which uses byte_masks
        // (byte_masks already encode case-insensitive matching).
        if budget == 0 && !self.case_insensitive {
            return text != self.pattern.as_bytes();
        }

        // For large budgets, the Bitap check itself becomes expensive.
        // Cap at 32 levels; beyond that, fall back to full DP.
        if budget > 32 {
            return false;
        }

        // Non-ASCII: need char-level masks, fall back to DP
        if !self.is_ascii {
            return false;
        }

        let d = budget;

        // Run Damerau-Levenshtein Bitap (with transposition) on the text slice.
        // R[i] bit j = 0 if pattern[0..j] matches some suffix of text with ≤ i errors.
        let mut r: [u128; 33] = [!0u128; 33];
        let mut old_r: [u128; 33] = [!0u128; 33];
        let mut old_old_r: [u128; 33] = [!0u128; 33];
        let mut prev_mask: Option<u128> = None;

        // Initialize: R[i] = all 1s shifted left by i
        for i in 1..=d {
            r[i] = r[i - 1] << 1;
        }

        for &byte in text {
            // Non-ASCII byte in text: can't use byte_masks (only 128 entries)
            if byte >= 128 {
                return false;
            }

            // Save states (shift history back by one step)
            old_old_r[..=d].copy_from_slice(&old_r[..=d]);
            old_r[..=d].copy_from_slice(&r[..=d]);

            let char_mask = self.byte_masks[byte as usize];

            // R[0] = exact match level
            r[0] = (old_r[0] << 1) | char_mask;

            // R[dd] = match with dd errors (Damerau-Levenshtein)
            for dd in 1..=d {
                let insert = old_r[dd - 1];
                let delete = r[dd - 1] << 1;
                let substitute = old_r[dd - 1] << 1;
                let match_d = (old_r[dd] << 1) | char_mask;
                let mut new_r = match_d & insert & delete & substitute;

                // Transposition: swap two adjacent pattern characters
                if let Some(pm) = prev_mask {
                    // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
                    let trans_valid_mask = char_mask | (pm >> 1);
                    // From matched position k, we can reach k+2 via transposition at k+1
                    let trans = ((old_old_r[dd - 1] << 1) | trans_valid_mask) << 1;
                    new_r &= trans;
                }

                r[dd] = new_r;
            }

            prev_mask = Some(char_mask);
        }

        // If accept bit (pattern_len - 1) is 1 in R[d], no match with ≤ d errors.
        (r[d] & self.accept_mask) != 0
    }

    /// Compute exact Damerau-Levenshtein edit breakdown using dynamic programming.
    /// Returns (insertions, deletions, substitutions, swaps).
    ///
    /// Optimized version using:
    /// - 3-row rotation instead of full O(m×n) table (for transposition support)
    /// - Stack allocation for small patterns (no heap allocation in common case)
    fn compute_exact_edit_breakdown(&self, matched_text: &[u8]) -> (u8, u8, u8, u8) {
        let pattern = &self.pattern_chars;
        let m = pattern.len();

        // Parse text as UTF-8
        let Ok(text_str) = std::str::from_utf8(matched_text) else {
            return (0, m as u8, 0, 0);
        };

        if m == 0 {
            let n = text_str.chars().count();
            return (n as u8, 0, 0, 0);
        }
        if text_str.is_empty() {
            return (0, m as u8, 0, 0);
        }

        // For small text, use fully stack-allocated version (common case)
        // Stack limit chosen to cover pattern_len <= 64 + typical edits
        const STACK_LIMIT: usize = 72;

        // For ASCII text, byte length == char count (fast path)
        let is_ascii = text_str.is_ascii();
        let n = if is_ascii {
            text_str.len()
        } else {
            text_str.chars().count()
        };

        // Always run the exact DP: it yields a valid Damerau breakdown whose
        // op counts describe a real alignment (`insertions - deletions ==
        // char_len - pattern_len`). A previous Myers early-reject returned
        // `(myers_dist, 0, 0, 0)` for over-budget windows, which is *not* a valid
        // breakdown (Levenshtein rather than Damerau, and an impossible
        // ins/del split). It only ever fired when the window was genuinely over
        // budget, so removing it never changes accept/reject outcomes; callers
        // that reject on `total_edits > limit` still reject (Damerau distance is
        // also over budget), but now with a consistent breakdown. Windows are
        // shorter than STACK_LIMIT, so the distance stays well within `u8`.
        if n < STACK_LIMIT {
            self.compute_edit_breakdown_small::<STACK_LIMIT>(pattern, text_str, m, n)
        } else {
            self.compute_edit_breakdown_large(pattern, text_str, m, n)
        }
    }

    /// Optimized DP for small text (stack allocated, 3-row rotation).
    #[inline]
    fn compute_edit_breakdown_small<const N: usize>(
        &self,
        pattern: &[char],
        text_str: &str,
        m: usize,
        n: usize,
    ) -> (u8, u8, u8, u8) {
        debug_assert!(n < N);

        // 3 rows for rotation: prev_prev (i-2), prev (i-1), curr (i)
        // Each row has n+1 elements
        type Cell = (u8, u8, u8, u8, u8); // (dist, ins, del, sub, swap)
        let mut prev_prev: [Cell; N] = [(0, 0, 0, 0, 0); N];
        let mut prev: [Cell; N] = [(0, 0, 0, 0, 0); N];
        let mut curr: [Cell; N] = [(0, 0, 0, 0, 0); N];

        // Stack-allocated text chars buffer (avoids heap allocation)
        let mut text_chars_buf: [char; N] = ['\0'; N];
        for (idx, c) in text_str.chars().take(N).enumerate() {
            text_chars_buf[idx] = if self.case_insensitive {
                c.to_ascii_lowercase()
            } else {
                c
            };
        }
        let text_chars = &text_chars_buf[..n];

        // Initialize row 0 (base case: insert j chars from text)
        // This goes into prev since the loop starts at i=1 and uses prev for i-1
        for j in 0..=n {
            prev[j] = (j as u8, j as u8, 0, 0, 0);
        }

        let mut prev_pattern_char = '\0';

        for i in 1..=m {
            let pattern_char = if self.case_insensitive {
                pattern[i - 1].to_ascii_lowercase()
            } else {
                pattern[i - 1]
            };

            // Base case for column 0: delete i chars from pattern
            curr[0] = (i as u8, 0, i as u8, 0, 0);

            for j in 1..=n {
                let text_char = text_chars[j - 1];

                if pattern_char == text_char {
                    // Match - no edit needed
                    curr[j] = prev[j - 1];
                } else {
                    // Try substitution (from prev[j-1])
                    let (sub_d, sub_i, sub_del, sub_s, sub_sw) = prev[j - 1];
                    let mut best = (sub_d + 1, sub_i, sub_del, sub_s + 1, sub_sw);

                    // Try insertion (from curr[j-1])
                    let (ins_d, ins_i, ins_del, ins_s, ins_sw) = curr[j - 1];
                    if ins_d + 1 < best.0 {
                        best = (ins_d + 1, ins_i + 1, ins_del, ins_s, ins_sw);
                    }

                    // Try deletion (from prev[j])
                    let (del_d, del_i, del_del, del_s, del_sw) = prev[j];
                    if del_d + 1 < best.0 {
                        best = (del_d + 1, del_i, del_del + 1, del_s, del_sw);
                    }

                    // Try transposition (from prev_prev[j-2])
                    if i > 1 && j > 1 {
                        let prev_text_char = text_chars[j - 2];
                        if pattern_char == prev_text_char && prev_pattern_char == text_char {
                            let (tr_d, tr_i, tr_del, tr_s, tr_sw) = prev_prev[j - 2];
                            if tr_d + 1 < best.0 {
                                best = (tr_d + 1, tr_i, tr_del, tr_s, tr_sw + 1);
                            }
                        }
                    }

                    curr[j] = best;
                }
            }

            // Rotate rows: prev_prev <- prev <- curr
            std::mem::swap(&mut prev_prev, &mut prev);
            std::mem::swap(&mut prev, &mut curr);
            prev_pattern_char = pattern_char;
        }

        // Result is in prev[n] (after final rotation)
        let (_, ins, del, sub, sw) = prev[n];
        (ins, del, sub, sw)
    }

    /// Fallback DP for large text (heap allocated).
    fn compute_edit_breakdown_large(
        &self,
        pattern: &[char],
        text_str: &str,
        m: usize,
        n: usize,
    ) -> (u8, u8, u8, u8) {
        type Cell = (u8, u8, u8, u8, u8);

        // 3 rows for rotation
        let mut prev_prev: Vec<Cell> = vec![(0, 0, 0, 0, 0); n + 1];
        let mut prev: Vec<Cell> = vec![(0, 0, 0, 0, 0); n + 1];
        let mut curr: Vec<Cell> = vec![(0, 0, 0, 0, 0); n + 1];

        // Initialize row 0
        for j in 0..=n {
            prev[j] = (j as u8, j as u8, 0, 0, 0);
        }

        let text_chars: Vec<char> = if self.case_insensitive {
            text_str.chars().map(|c| c.to_ascii_lowercase()).collect()
        } else {
            text_str.chars().collect()
        };

        let mut prev_pattern_char = '\0';

        for i in 1..=m {
            let pattern_char = if self.case_insensitive {
                pattern[i - 1].to_ascii_lowercase()
            } else {
                pattern[i - 1]
            };

            curr[0] = (i as u8, 0, i as u8, 0, 0);

            for j in 1..=n {
                let text_char = text_chars[j - 1];

                if pattern_char == text_char {
                    curr[j] = prev[j - 1];
                } else {
                    let (sub_d, sub_i, sub_del, sub_s, sub_sw) = prev[j - 1];
                    let mut best = (sub_d + 1, sub_i, sub_del, sub_s + 1, sub_sw);

                    let (ins_d, ins_i, ins_del, ins_s, ins_sw) = curr[j - 1];
                    if ins_d + 1 < best.0 {
                        best = (ins_d + 1, ins_i + 1, ins_del, ins_s, ins_sw);
                    }

                    let (del_d, del_i, del_del, del_s, del_sw) = prev[j];
                    if del_d + 1 < best.0 {
                        best = (del_d + 1, del_i, del_del + 1, del_s, del_sw);
                    }

                    if i > 1 && j > 1 {
                        let prev_text_char = text_chars[j - 2];
                        if pattern_char == prev_text_char && prev_pattern_char == text_char {
                            let (tr_d, tr_i, tr_del, tr_s, tr_sw) = prev_prev[j - 2];
                            if tr_d + 1 < best.0 {
                                best = (tr_d + 1, tr_i, tr_del, tr_s, tr_sw + 1);
                            }
                        }
                    }

                    curr[j] = best;
                }
            }

            std::mem::swap(&mut prev_prev, &mut prev);
            std::mem::swap(&mut prev, &mut curr);
            prev_pattern_char = pattern_char;
        }

        let (_, ins, del, sub, sw) = prev[n];
        (ins, del, sub, sw)
    }

    /// Find the first match in the text.
    ///
    /// This delegates to `find_all_non_overlapping` and returns the first result.
    /// While not optimal for all cases, this ensures correct behavior for edge cases
    /// like transpositions and complex fuzzy matches.
    #[must_use]
    pub fn find_first(&self, text: &str, threshold: f32) -> Option<DamLevMatch> {
        // Delegate to the well-tested find_all_non_overlapping
        // require_first_char=false allows matches where first char is edited
        let matches = self.find_all_non_overlapping(text, threshold, false);
        matches.into_iter().min_by_key(|m| m.start)
    }

    /// Find first match starting from candidate positions only.
    #[must_use]
    pub fn find_first_with_candidates(
        &self,
        text: &str,
        threshold: f32,
        candidates: &super::hash::FxHashSet<usize>,
    ) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        let effective_max = max_edits.min(self.pattern_len);
        let text_chars: Vec<(usize, char)> = text.char_indices().collect();

        if text_chars.is_empty() || candidates.is_empty() {
            return None;
        }

        // For each candidate position, run a localized Bitap search
        let mut sorted_candidates: Vec<usize> = candidates.iter().copied().collect();
        sorted_candidates.sort_unstable();

        // Pre-allocate state buffers outside the loop (reused across candidates)
        let mut r: Vec<u128> = vec![!0u128; effective_max + 1];
        let mut old_r: Vec<u128> = vec![!0u128; effective_max + 1];

        for &start_byte in &sorted_candidates {
            // Find the character index for this byte position using binary search (O(log N))
            let start_char = text_chars
                .binary_search_by_key(&start_byte, |(b, _)| *b)
                .unwrap_or(0);

            // Reset state for this candidate
            r.fill(!0u128);

            // Initialize deletion states - left shift advances pattern position
            for d in 1..=effective_max {
                r[d] = r[d - 1] << 1;
            }

            let max_window = self.pattern_len + effective_max;

            // Track best match within this window
            let mut best_match: Option<(usize, DamLevMatch)> = None;

            for (rel_idx, &(_, text_char)) in text_chars[start_char..]
                .iter()
                .enumerate()
                .take(max_window + 1)
            {
                let text_char = if self.case_insensitive {
                    text_char.to_lowercase().next().unwrap_or(text_char)
                } else {
                    text_char
                };

                let char_mask = self.get_mask(text_char);

                // Swap buffers: old_r gets previous r (no allocation!)
                std::mem::swap(&mut r, &mut old_r);

                r[0] = (old_r[0] << 1) | char_mask;

                for d in 1..=effective_max {
                    let insert = old_r[d - 1];
                    let delete = r[d - 1] << 1; // left shift advances pattern position
                    let substitute = old_r[d - 1] << 1;
                    let match_d = (old_r[d] << 1) | char_mask;

                    r[d] = match_d & insert & delete & substitute;
                }

                // Check for match
                let abs_idx = start_char + rel_idx;
                let end_byte = text_chars.get(abs_idx + 1).map_or(text.len(), |(b, _)| *b);

                for d in 0..=effective_max {
                    if (r[d] & self.accept_mask) == 0 {
                        // Pre-filter: skip error levels where the best possible
                        // similarity is below threshold.
                        let sim_max = 1.0f32 - d as f32 / (self.pattern_len + d) as f32;
                        if sim_max < threshold {
                            continue;
                        }
                        // Compute exact edit breakdown using DP
                        let text_slice = &text.as_bytes()[start_byte..end_byte];
                        // Fast early-reject via Levenshtein-only Bitap
                        if self.quick_reject(text_slice, d) {
                            continue;
                        }
                        let (insertions, deletions, substitutions, swaps) = self
                            .compute_exact_edit_breakdown(text_slice);

                        let sim = self.calc_similarity(d as u8, insertions, deletions);
                        if sim >= threshold {
                            let candidate = DamLevMatch {
                                start: start_byte,
                                end: end_byte,
                                insertions,
                                deletions,
                                substitutions,
                                swaps,
                                similarity: sim,
                            };

                            // Update best if this has fewer edits
                            let dominated =
                                best_match.as_ref().is_some_and(|(best_d, _)| *best_d <= d);
                            if !dominated {
                                best_match = Some((d, candidate));
                            }

                            // If exact match found, return immediately
                            if d == 0 {
                                return best_match.map(|(_, m)| m);
                            }
                        }
                    }
                }
            }

            // Return best match from this candidate window if found
            if let Some((_, m)) = best_match {
                return Some(m);
            }
        }

        None
    }

    /// Ultra-fast search starting from a specific byte position.
    ///
    /// This method is optimized for the greedy-first hot path:
    /// - No allocations (uses stack arrays for small k)
    /// - Direct byte iteration
    /// - Early termination on first match
    /// - SIMD acceleration when available (`AVX2` on `x86_64`)
    #[inline]
    #[must_use]
    pub fn find_at_byte_position(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
    ) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;

        // Handle empty/exhausted text: pattern can still match via pure deletions
        if start_pos >= text.len() {
            // If pattern length <= max_edits, we can delete the entire pattern
            if self.pattern_len <= max_edits {
                let deletions = self.pattern_len as u8;
                let sim = self.calc_similarity(deletions, 0, deletions);
                if sim >= threshold {
                    return Some(DamLevMatch {
                        start: start_pos,
                        end: start_pos,
                        insertions: 0,
                        deletions,
                        substitutions: 0,
                        swaps: 0,
                        similarity: sim,
                    });
                }
            }
            return None;
        }

        // SIMD fast path: NEON on aarch64 for ASCII patterns with k <= 1
        #[cfg(all(feature = "simd", target_arch = "aarch64"))]
        {
            if self.is_ascii && max_edits <= 1 && self.pattern_len <= FAST_PATH_MAX_LEN {
                // SAFETY: NEON is mandatory on aarch64
                return unsafe {
                    self.find_at_byte_position_neon(text, start_pos, threshold, max_edits)
                };
            }
        }

        // SIMD fast path: AVX2 on x86_64 for ASCII patterns with k <= 3
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        {
            if self.is_ascii && max_edits <= 3 && simd_avx2::is_available() && self.pattern_len <= FAST_PATH_MAX_LEN {
                // SAFETY: We've verified AVX2 is available via runtime detection
                return unsafe {
                    self.find_at_byte_position_avx2(text, start_pos, threshold, max_edits)
                };
            }
        }

        // Use ASCII fast path when pattern is ASCII
        // This avoids UTF-8 decoding and uses direct byte array lookup
        if self.is_ascii && max_edits <= 4 && self.pattern_len <= FAST_PATH_MAX_LEN {
            return self.find_at_byte_position_ascii::<5>(text, start_pos, threshold);
        }

        // Use stack array for small k (common case), fall back to vec for large k
        if max_edits <= 4 && self.pattern_len <= FAST_PATH_MAX_LEN {
            self.find_at_byte_position_small_k::<5>(text, start_pos, threshold)
        } else {
            self.find_at_byte_position_large_k(text, start_pos, threshold)
        }
    }

    /// NEON-accelerated search for ASCII patterns with k <= 1.
    ///
    /// # Safety
    /// Safe on all aarch64 targets (NEON is mandatory).
    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    #[inline]
    unsafe fn find_at_byte_position_neon(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
        max_edits: usize,
    ) -> Option<DamLevMatch> {
        debug_assert!(max_edits <= 1);
        debug_assert!(self.is_ascii);

        let max_window = self.pattern_len + max_edits;
        let end_limit = (start_pos + max_window + 1).min(text.len());
        let search_len = end_limit - start_pos;

        if search_len == 0 {
            return None;
        }

        // State arrays: r = current, old_r = previous, old_old_r = 2 iterations ago (for transposition)
        let mut r = [!0u64; 4];
        let mut old_r = [!0u64; 4];

        // Initialize deletion states - left shift advances pattern position
        for d in 1..=max_edits {
            r[d] = r[d - 1] << 1;
        }

        // SAFETY: start_pos < text.len() verified by caller, search_len bounds checked above
        let text_ptr = unsafe { text.as_ptr().add(start_pos) };
        let byte_masks_ptr = self.byte_masks.as_ptr();
        let accept_mask = self.accept_mask as u64;
        let mut prev_mask: u64 = !0u64;
        let mut old_old_r = old_r;

        for i in 0..search_len {
            // SAFETY: i < search_len which is bounded by text.len() - start_pos
            let byte = unsafe { *text_ptr.add(i) };
            let mask_idx = (byte & 0x7F) as usize;
            // SAFETY: mask_idx is always < 128 due to & 0x7F, and byte_masks has 128 elements
            let char_mask: u64 = unsafe { *byte_masks_ptr.add(mask_idx) as u64 };
            // Rotate state history before update
            std::mem::swap(&mut old_old_r, &mut old_r);
            old_r = r;

            // Use NEON state update
            // SAFETY: NEON is mandatory on aarch64
            unsafe {
                simd_neon::update_states_with_trans_k1_neon(
                    &mut r, &old_r, &old_old_r, char_mask, prev_mask,
                );
            }

            let char_count = i + 1;

            for d in 0..=max_edits {
                if (r[d] & accept_mask) == 0 {
                    let end_byte = start_pos + char_count;
                    let (insertions, deletions, substitutions, swaps) =
                        self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                    let sim = self.calc_similarity(d as u8, insertions, deletions);
                    if sim >= threshold {
                        return Some(DamLevMatch {
                            start: start_pos,
                            end: end_byte,
                            insertions,
                            deletions,
                            substitutions,
                            swaps,
                            similarity: sim,
                        });
                    }
                }
            }

            prev_mask = char_mask;
        }

        // Handle text shorter than pattern: positions reached during the last iteration
        // need extra match propagation to reach the accept position.
        let end_byte = start_pos + search_len;
        let chars_short = self.pattern_len.saturating_sub(search_len);
        if chars_short > 0 && prev_mask != !0u64 {
            for _ in 0..chars_short.min(max_edits) {
                old_r = r;

                // Apply match propagation with last char's mask
                for d in 1..=max_edits {
                    let match_d = (old_r[d] << 1) | prev_mask;
                    r[d] &= match_d;
                }

                // Check for accept
                for d in 0..=max_edits {
                    if (r[d] & accept_mask) == 0 {
                        let (insertions, deletions, substitutions, swaps) =
                            self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                        let total = insertions + deletions + substitutions + swaps;
                        if total as usize <= d {
                            let sim = self.calc_similarity(total, insertions, deletions);
                            if sim >= threshold {
                                return Some(DamLevMatch {
                                    start: start_pos,
                                    end: end_byte,
                                    insertions,
                                    deletions,
                                    substitutions,
                                    swaps,
                                    similarity: sim,
                                });
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// AVX2-accelerated search for ASCII patterns with k <= 3.
    ///
    /// # Safety
    /// Caller must ensure AVX2 is available (check with `simd_avx2::is_available()`).
    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn find_at_byte_position_avx2(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
        max_edits: usize,
    ) -> Option<DamLevMatch> {
        debug_assert!(max_edits <= 3);
        debug_assert!(self.is_ascii);

        let max_window = self.pattern_len + max_edits;
        let end_limit = (start_pos + max_window + 1).min(text.len());
        let search_len = end_limit - start_pos;

        if search_len == 0 {
            return None;
        }

        // State arrays (4 elements for k <= 3)
        let mut r = [!0u64; 4];
        let mut old_r = [!0u64; 4];
        #[allow(unused_assignments)]
        let mut old_old_r = [!0u64; 4];

        // Initialize deletion states - left shift advances pattern position
        for d in 1..=max_edits {
            r[d] = r[d - 1] << 1;
        }

        // SAFETY: start_pos < text.len() verified by caller, search_len bounds checked above
        let text_ptr = unsafe { text.as_ptr().add(start_pos) };
        let byte_masks_ptr = self.byte_masks.as_ptr();
        let accept_mask = self.accept_mask as u64;

        let mut prev_mask: u64 = !0u64;

        for i in 0..search_len {
            // SAFETY: i < search_len which is bounded by text.len() - start_pos
            let byte = unsafe { *text_ptr.add(i) };

            // Direct array lookup for ASCII
            let mask_idx = (byte & 0x7F) as usize;
            // SAFETY: mask_idx is always < 128 due to & 0x7F, and byte_masks has 128 elements
            // SAFETY: mask_idx is always < 128 due to & 0x7F, and byte_masks has 128 elements
            let char_mask: u64 = unsafe { *byte_masks_ptr.add(mask_idx) as u64 };
            // Save old states
            old_old_r = old_r;
            old_r = r;

            // Use SIMD state update with transposition
            // SAFETY: AVX2 availability verified by caller via simd_avx2::is_available()
            unsafe {
                simd_avx2::update_states_with_trans_avx2(
                    &mut r, &old_r, &old_old_r, char_mask, prev_mask, max_edits,
                );
            }

            let char_count = i + 1;

            // Check for match (prefer fewer edits)
            for d in 0..=max_edits {
                if (r[d] & accept_mask) == 0 {
                    let end_byte = start_pos + char_count;

                    // Compute exact edit breakdown using DP
                    let (insertions, deletions, substitutions, swaps) =
                        self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                    let sim = self.calc_similarity(d as u8, insertions, deletions);
                    if sim >= threshold {
                        return Some(DamLevMatch {
                            start: start_pos,
                            end: end_byte,
                            insertions,
                            deletions,
                            substitutions,
                            swaps,
                            similarity: sim,
                        });
                    }
                }
            }

            prev_mask = char_mask;
        }

        // Handle text shorter than pattern: positions reached during the last iteration
        // need extra match propagation to reach the accept position.
        let end_byte = start_pos + search_len;
        let chars_short = self.pattern_len.saturating_sub(search_len);
        if chars_short > 0 && prev_mask != !0u64 {
            for _ in 0..chars_short.min(max_edits) {
                old_r = r;

                // Apply match propagation with last char's mask
                for d in 1..=max_edits {
                    let match_d = (old_r[d] << 1) | prev_mask;
                    r[d] &= match_d;
                }

                // Check for accept
                for d in 0..=max_edits {
                    if (r[d] & accept_mask) == 0 {
                        let (insertions, deletions, substitutions, swaps) =
                            self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                        let total = insertions + deletions + substitutions + swaps;
                        if total as usize <= d {
                            let sim = self.calc_similarity(total, insertions, deletions);
                            if sim >= threshold {
                                return Some(DamLevMatch {
                                    start: start_pos,
                                    end: end_byte,
                                    insertions,
                                    deletions,
                                    substitutions,
                                    swaps,
                                    similarity: sim,
                                });
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Search multiple positions in parallel using SIMD.
    ///
    /// Processes up to 4 candidate positions simultaneously, returning the first match.
    /// This avoids the cascade dependency issue by parallelizing across positions
    /// rather than across error levels.
    ///
    /// Returns `Some((position_index, match))` if a match is found.
    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    #[inline]
    #[must_use]
    pub fn find_at_positions_parallel(
        &self,
        text: &[u8],
        positions: &[usize],
        threshold: f32,
    ) -> Option<(usize, DamLevMatch)> {
        if positions.is_empty() || !self.is_ascii {
            return None;
        }

        let max_edits = self.limits.max_edits as usize;

        // NEON processes 2 positions at a time for k=0
        if max_edits == 0 {
            // Process pairs of positions
            let mut i = 0;
            while i + 1 < positions.len() {
                let pos_pair = [positions[i], positions[i + 1]];
                if let Some((idx, m)) =
                    unsafe { self.find_at_2_positions_neon_k0(text, pos_pair, threshold) }
                {
                    return Some((i + idx, m));
                }
                i += 2;
            }
            // Handle remaining position
            if i < positions.len()
                && let Some(m) = self.find_at_byte_position(text, positions[i], threshold)
            {
                return Some((i, m));
            }
            return None;
        }

        // For k >= 1, fall back to sequential (NEON k=1 is already optimized)
        for (i, &pos) in positions.iter().enumerate() {
            if let Some(m) = self.find_at_byte_position(text, pos, threshold) {
                return Some((i, m));
            }
        }
        None
    }

    /// Search multiple positions in parallel using AVX2.
    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    #[inline]
    #[must_use]
    pub fn find_at_positions_parallel(
        &self,
        text: &[u8],
        positions: &[usize],
        threshold: f32,
    ) -> Option<(usize, DamLevMatch)> {
        if positions.is_empty() || !self.is_ascii {
            return None;
        }

        let max_edits = self.limits.max_edits as usize;

        // AVX2 processes 4 positions at a time for k=0
        if max_edits == 0 && simd_avx2::is_available() {
            let mut i = 0;
            while i + 3 < positions.len() {
                let pos_quad = [
                    positions[i],
                    positions[i + 1],
                    positions[i + 2],
                    positions[i + 3],
                ];
                if let Some((idx, m)) =
                    unsafe { self.find_at_4_positions_avx2_k0(text, pos_quad, threshold) }
                {
                    return Some((i + idx, m));
                }
                i += 4;
            }
            // Handle remaining positions
            while i < positions.len() {
                if let Some(m) = self.find_at_byte_position(text, positions[i], threshold) {
                    return Some((i, m));
                }
                i += 1;
            }
            return None;
        }

        // Fall back to sequential for k >= 1 or no AVX2
        for (i, &pos) in positions.iter().enumerate() {
            if let Some(m) = self.find_at_byte_position(text, pos, threshold) {
                return Some((i, m));
            }
        }
        None
    }

    /// Fallback for non-SIMD builds
    #[cfg(not(any(
        all(feature = "simd", target_arch = "aarch64"),
        all(feature = "simd", target_arch = "x86_64")
    )))]
    #[inline]
    pub fn find_at_positions_parallel(
        &self,
        text: &[u8],
        positions: &[usize],
        threshold: f32,
    ) -> Option<(usize, DamLevMatch)> {
        for (i, &pos) in positions.iter().enumerate() {
            if let Some(m) = self.find_at_byte_position(text, pos, threshold) {
                return Some((i, m));
            }
        }
        None
    }

    /// NEON: Search 2 positions in parallel for k=0 (exact match).
    ///
    /// Uses 128-bit NEON to process 2 independent exact-match searches.
    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    #[inline]
    unsafe fn find_at_2_positions_neon_k0(
        &self,
        text: &[u8],
        positions: [usize; 2],
        threshold: f32,
    ) -> Option<(usize, DamLevMatch)> {
        #[allow(clippy::wildcard_imports)]
        use std::arch::aarch64::*;

        let max_window = self.pattern_len;
        let accept_mask = self.accept_mask as u64;
        let byte_masks = &self.byte_masks;

        // Calculate search lengths for each position
        let end0 = (positions[0] + max_window + 1).min(text.len());
        let end1 = (positions[1] + max_window + 1).min(text.len());
        let len0 = end0.saturating_sub(positions[0]);
        let len1 = end1.saturating_sub(positions[1]);
        let max_len = len0.max(len1);

        if max_len == 0 {
            return None;
        }

        // Initialize states: all 1s means no match yet
        // r[0] = position 0 state, r[1] = position 1 state
        let mut r = unsafe { vdupq_n_u64(!0u64) };
        let accept_vec = unsafe { vdupq_n_u64(accept_mask) };

        for i in 0..max_len {
            // Get char masks for both positions (scalar loads, then combine)
            let mask0: u64 = if i < len0 {
                let byte = text[positions[0] + i];
                byte_masks[(byte & 0x7F) as usize] as u64
            } else {
                !0u64 // No match possible
            };

            let mask1: u64 = if i < len1 {
                let byte = text[positions[1] + i];
                byte_masks[(byte & 0x7F) as usize] as u64
            } else {
                !0u64
            };

            // Combine masks into NEON vector
            let char_masks = unsafe { vcombine_u64(vcreate_u64(mask0), vcreate_u64(mask1)) };

            // State update: r = (r << 1) | char_mask
            let shifted = unsafe { vshlq_n_u64(r, 1) };
            r = unsafe { vorrq_u64(shifted, char_masks) };

            // Check for matches: (r & accept_mask) == 0
            let masked = unsafe { vandq_u64(r, accept_vec) };

            // Extract and check each lane
            let lane0 = unsafe { vgetq_lane_u64(masked, 0) };
            let lane1 = unsafe { vgetq_lane_u64(masked, 1) };

            if lane0 == 0 && i < len0 {
                let end_byte = positions[0] + i + 1;
                let (insertions, deletions, substitutions, swaps) =
                    self.compute_exact_edit_breakdown(&text[positions[0]..end_byte]);
                let sim = self.calc_similarity(0, insertions, deletions);
                if sim >= threshold {
                    return Some((
                        0,
                        DamLevMatch {
                            start: positions[0],
                            end: end_byte,
                            insertions,
                            deletions,
                            substitutions,
                            swaps,
                            similarity: sim,
                        },
                    ));
                }
            }

            if lane1 == 0 && i < len1 {
                let end_byte = positions[1] + i + 1;
                let (insertions, deletions, substitutions, swaps) =
                    self.compute_exact_edit_breakdown(&text[positions[1]..end_byte]);
                let sim = self.calc_similarity(0, insertions, deletions);
                if sim >= threshold {
                    return Some((
                        1,
                        DamLevMatch {
                            start: positions[1],
                            end: end_byte,
                            insertions,
                            deletions,
                            substitutions,
                            swaps,
                            similarity: sim,
                        },
                    ));
                }
            }
        }

        None
    }

    /// AVX2: Search 4 positions in parallel for k=0 (exact match).
    ///
    /// Uses 256-bit AVX2 to process 4 independent exact-match searches.
    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn find_at_4_positions_avx2_k0(
        &self,
        text: &[u8],
        positions: [usize; 4],
        threshold: f32,
    ) -> Option<(usize, DamLevMatch)> {
        #[allow(clippy::wildcard_imports)]
        use std::arch::x86_64::*;

        let max_window = self.pattern_len;
        let accept_mask = self.accept_mask as u64;
        let byte_masks = &self.byte_masks;

        // Calculate search lengths for each position
        let ends: [usize; 4] = [
            (positions[0] + max_window + 1).min(text.len()),
            (positions[1] + max_window + 1).min(text.len()),
            (positions[2] + max_window + 1).min(text.len()),
            (positions[3] + max_window + 1).min(text.len()),
        ];
        let lens: [usize; 4] = [
            ends[0].saturating_sub(positions[0]),
            ends[1].saturating_sub(positions[1]),
            ends[2].saturating_sub(positions[2]),
            ends[3].saturating_sub(positions[3]),
        ];
        let max_len = lens[0].max(lens[1]).max(lens[2]).max(lens[3]);

        if max_len == 0 {
            return None;
        }

        // Initialize states: all 1s
        let mut r = _mm256_set1_epi64x(!0i64);
        let accept_vec = _mm256_set1_epi64x(accept_mask as i64);

        for i in 0..max_len {
            // Get char masks for all 4 positions
            let masks: [u64; 4] = [
                if i < lens[0] {
                    byte_masks[(text[positions[0] + i] & 0x7F) as usize] as u64
                } else {
                    !0u64
                },
                if i < lens[1] {
                    byte_masks[(text[positions[1] + i] & 0x7F) as usize] as u64
                } else {
                    !0u64
                },
                if i < lens[2] {
                    byte_masks[(text[positions[2] + i] & 0x7F) as usize] as u64
                } else {
                    !0u64
                },
                if i < lens[3] {
                    byte_masks[(text[positions[3] + i] & 0x7F) as usize] as u64
                } else {
                    !0u64
                },
            ];

            let char_masks = _mm256_set_epi64x(
                masks[3] as i64,
                masks[2] as i64,
                masks[1] as i64,
                masks[0] as i64,
            );

            // State update: r = (r << 1) | char_mask
            let shifted = _mm256_slli_epi64(r, 1);
            r = _mm256_or_si256(shifted, char_masks);

            // Check for matches
            let masked = _mm256_and_si256(r, accept_vec);

            // Extract and check each lane (use movemask for efficiency)
            let zero = _mm256_setzero_si256();
            let cmp = _mm256_cmpeq_epi64(masked, zero);
            let match_mask = _mm256_movemask_epi8(cmp);

            // Check each position (lanes are in order: 0, 1, 2, 3)
            // Each lane is 8 bytes, so bits 0-7 = lane 0, 8-15 = lane 1, etc.
            if match_mask != 0 {
                for (lane, &len) in lens.iter().enumerate() {
                    if i < len {
                        let lane_mask = 0xFF << (lane * 8);
                        if (match_mask & lane_mask) == lane_mask {
                            let end_byte = positions[lane] + i + 1;
                            let (insertions, deletions, substitutions, swaps) =
                                self.compute_exact_edit_breakdown(&text[positions[lane]..end_byte]);
                            let sim = self.calc_similarity(0, insertions, deletions);
                            if sim >= threshold {
                                return Some((
                                    lane,
                                    DamLevMatch {
                                        start: positions[lane],
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// ASCII-optimized search - no UTF-8 decoding, direct byte mask lookup.
    /// Uses unsafe to eliminate bounds checks in the hot loop.
    #[inline(always)]
    fn find_at_byte_position_ascii<const K: usize>(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
    ) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        debug_assert!(max_edits < K);

        let max_window = self.pattern_len + max_edits;
        let end_limit = (start_pos + max_window + 1).min(text.len());
        let search_len = end_limit - start_pos;

        if search_len == 0 {
            return None;
        }

        // SAFETY: We've bounds-checked above, and byte_masks has 128 elements
        // which covers all ASCII bytes (0-127). Non-ASCII bytes are handled
        // by returning !0u128 (no match).
        unsafe {
            self.find_at_byte_position_ascii_unchecked::<K>(
                text, start_pos, search_len, threshold, max_edits,
            )
        }
    }

    /// Inner loop with no bounds checks - SAFETY: caller must ensure bounds are valid
    #[inline(always)]
    unsafe fn find_at_byte_position_ascii_unchecked<const K: usize>(
        &self,
        text: &[u8],
        start_pos: usize,
        search_len: usize,
        threshold: f32,
        max_edits: usize,
    ) -> Option<DamLevMatch> {
        // SAFETY: caller guarantees all bounds are valid
        unsafe {
            // Stack-allocated state vectors
            let mut r = [!0u128; K];
            let mut old_r = [!0u128; K];
            let mut old_old_r = [!0u128; K]; // State from 2 iterations ago for transposition

            // Initialize deletion states - left shift advances pattern position
            for d in 1..=max_edits {
                *r.get_unchecked_mut(d) = *r.get_unchecked(d - 1) << 1;
            }

            let text_ptr = text.as_ptr().add(start_pos);
            let byte_masks_ptr = self.byte_masks.as_ptr();
            let accept_mask = self.accept_mask;
            let _ = self.pattern_len;

            let mut prev_byte: Option<u8> = None;

            for i in 0..search_len {
                let byte = *text_ptr.add(i);

                // Direct array lookup - mask non-ASCII to 0 index (which has !0u128)
                let mask_idx = (byte & 0x7F) as usize;
                let char_mask = *byte_masks_ptr.add(mask_idx);

                // Save states from 2 iterations ago
                for d in 0..=max_edits {
                    *old_old_r.get_unchecked_mut(d) = *old_r.get_unchecked(d);
                    *old_r.get_unchecked_mut(d) = *r.get_unchecked(d);
                }

                // Update state 0 (exact match)
                *r.get_unchecked_mut(0) = (*r.get_unchecked(0) << 1) | char_mask;

                // Update fuzzy states
                for d in 1..=max_edits {
                    let insert = *old_r.get_unchecked(d - 1);
                    let delete = *r.get_unchecked(d - 1) << 1; // left shift advances pattern position
                    let substitute = *old_r.get_unchecked(d - 1) << 1;
                    let match_d = (*old_r.get_unchecked(d) << 1) | char_mask;
                    let mut new_r = match_d & insert & delete & substitute;

                    // Transposition: if we have a previous character, check for swaps
                    // Transposition at position j means pattern[j]=curr AND pattern[j+1]=prev
                    if let Some(prev_b) = prev_byte {
                        let prev_mask_idx = (prev_b & 0x7F) as usize;
                        let prev_mask = *byte_masks_ptr.add(prev_mask_idx);
                        // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
                        let trans_valid_mask = char_mask | (prev_mask >> 1);
                        // From matched position k (bit k=0), we can reach k+2 via transposition at k+1
                        // Shift old_old_r left first to align: bit k becomes bit k+1
                        // This also makes bit 0 = 0, allowing transposition at position 0
                        let trans =
                            ((*old_old_r.get_unchecked(d - 1) << 1) | trans_valid_mask) << 1;
                        new_r &= trans;
                    }

                    *r.get_unchecked_mut(d) = new_r;
                }

                let char_count = i + 1;

                // Check for match (prefer fewer edits)
                for d in 0..=max_edits {
                    if (*r.get_unchecked(d) & accept_mask) == 0 {
                        let end_byte = start_pos + char_count;
                        // Compute exact edit breakdown using DP
                        let (insertions, deletions, substitutions, swaps) =
                            self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                        let sim = self.calc_similarity(d as u8, insertions, deletions);
                        if sim >= threshold {
                            return Some(DamLevMatch {
                                start: start_pos,
                                end: end_byte,
                                insertions,
                                deletions,
                                substitutions,
                                swaps,
                                similarity: sim,
                            });
                        }
                    }
                }

                prev_byte = Some(byte);
            }

            // Handle text shorter than pattern: positions reached during the last iteration
            // need extra match propagation to reach the accept position.
            // Re-process the last char_mask to allow match propagation.
            let end_byte = start_pos + search_len;
            if let Some(last_byte) = prev_byte {
                let last_mask = *byte_masks_ptr.add((last_byte & 0x7F) as usize);
                let chars_short = self.pattern_len.saturating_sub(search_len);

                for _ in 0..chars_short.min(max_edits) {
                    for d in 0..=max_edits {
                        *old_r.get_unchecked_mut(d) = *r.get_unchecked(d);
                    }

                    // Apply match propagation: from position p with d errors,
                    // if pattern[p+1] matches last_char, reach position p+1 with d errors
                    for d in 1..=max_edits {
                        let match_d = (*old_r.get_unchecked(d) << 1) | last_mask;
                        *r.get_unchecked_mut(d) &= match_d;
                    }

                    // Check for accept after each propagation
                    for d in 0..=max_edits {
                        if (*r.get_unchecked(d) & accept_mask) == 0 {
                            let (insertions, deletions, substitutions, swaps) =
                                self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                            let total = insertions + deletions + substitutions + swaps;
                            if total as usize <= d {
                                let sim = self.calc_similarity(total, insertions, deletions);
                                if sim >= threshold {
                                    return Some(DamLevMatch {
                                        start: start_pos,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            None
        }
    }

    #[inline]
    fn find_at_byte_position_small_k<const K: usize>(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
    ) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        debug_assert!(max_edits < K);

        // Stack-allocated state vectors
        let mut r = [!0u128; K];
        let mut old_r = [!0u128; K];
        let mut old_old_r = [!0u128; K]; // State from 2 iterations ago for transposition

        // Initialize deletion states - left shift advances pattern position
        for d in 1..=max_edits {
            r[d] = r[d - 1] << 1;
        }

        // max_window is in characters, but we iterate bytes.
        // For UTF-8, multiply by max char size (4) to ensure we process enough bytes.
        let max_window_chars = self.pattern_len + max_edits;
        let max_window_bytes = if self.is_ascii {
            max_window_chars + 1
        } else {
            max_window_chars * 4 + 1
        };
        let end_limit = (start_pos + max_window_bytes).min(text.len());

        // Cache previous mask to avoid redundant lookups in transposition check
        let mut prev_mask: Option<u128> = None;
        let case_insensitive = self.case_insensitive;

        // Iterate bytes, handling UTF-8
        let mut pos = start_pos;
        let mut char_count = 0usize;
        while pos < end_limit && char_count <= max_window_chars {
            let byte = text[pos];

            // Get character mask and length with fast paths
            let (char_mask, char_len) = if byte < 128 {
                // ASCII fast path
                let lookup_byte = if case_insensitive {
                    byte.to_ascii_lowercase()
                } else {
                    byte
                };
                (self.byte_masks[lookup_byte as usize], 1)
            } else if byte < 224 && pos + 1 < text.len() {
                // 2-byte UTF-8 fast path (Cyrillic, etc.)
                let b1 = text[pos + 1];
                if case_insensitive {
                    let codepoint = ((u32::from(byte) & 0x1F) << 6) | (u32::from(b1) & 0x3F);
                    let ch = unsafe { char::from_u32_unchecked(codepoint) };
                    let ch_lower = ch.to_lowercase().next().unwrap_or(ch);
                    (self.get_mask(ch_lower), 2)
                } else {
                    (self.get_mask_2byte(byte, b1), 2)
                }
            } else {
                // 3/4-byte UTF-8 or incomplete
                let (ch, len) = decode_utf8_char_fast(text, pos);
                let ch = if case_insensitive {
                    ch.to_lowercase().next().unwrap_or(ch)
                } else {
                    ch
                };
                (self.get_mask(ch), len)
            };

            // Save old states
            old_old_r[..=max_edits].copy_from_slice(&old_r[..=max_edits]);
            old_r[..=max_edits].copy_from_slice(&r[..=max_edits]);

            // Update states
            r[0] = (r[0] << 1) | char_mask;

            for d in 1..=max_edits {
                let insert = old_r[d - 1];
                let delete = r[d - 1] << 1; // left shift advances pattern position
                let substitute = old_r[d - 1] << 1;
                let match_d = (old_r[d] << 1) | char_mask;
                let mut new_r = match_d & insert & delete & substitute;

                // Transposition: if we have a previous mask, check for swaps
                // (use cached prev_mask instead of recomputing)
                if let Some(pm) = prev_mask {
                    // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
                    let trans_valid_mask = char_mask | (pm >> 1);
                    // From matched position k, we can reach k+2 via transposition at k+1
                    let trans = ((old_old_r[d - 1] << 1) | trans_valid_mask) << 1;
                    new_r &= trans;
                }

                r[d] = new_r;
            }

            let end_byte = pos + char_len;

            // Check for match (prefer fewer edits)
            for d in 0..=max_edits {
                if (r[d] & self.accept_mask) == 0 {
                    // Compute exact edit breakdown using DP
                    let (insertions, deletions, substitutions, swaps) =
                        self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                    let sim = self.calc_similarity(d as u8, insertions, deletions);
                    if sim >= threshold {
                        return Some(DamLevMatch {
                            start: start_pos,
                            end: end_byte,
                            insertions,
                            deletions,
                            substitutions,
                            swaps,
                            similarity: sim,
                        });
                    }
                }
            }

            prev_mask = Some(char_mask);
            pos += char_len;
            char_count += 1;
        }

        // Handle text shorter than pattern: positions reached during the last iteration
        // need extra match propagation to reach the accept position.
        // Re-process the last char_mask to allow match propagation.
        let end_byte = pos;
        if let Some(last_mask) = prev_mask {
            let chars_short = self.pattern_len.saturating_sub(char_count);
            for _ in 0..chars_short.min(max_edits) {
                old_r = r;
                // Note: old_old_r is intentionally not updated - transpositions don't apply
                // when we're just propagating matches without processing new characters

                // Apply match propagation: from position p with d errors,
                // if pattern[p+1] matches last_char, reach position p+1 with d errors
                // Note: transpositions don't apply here since we're not processing new characters
                for d in 1..=max_edits {
                    let match_d = (old_r[d] << 1) | last_mask;
                    r[d] &= match_d;
                }

                // Check for accept after each propagation
                for d in 0..=max_edits {
                    if (r[d] & self.accept_mask) == 0 {
                        let (insertions, deletions, substitutions, swaps) =
                            self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                        let total = insertions + deletions + substitutions + swaps;
                        if total as usize <= d {
                            let sim = self.calc_similarity(total, insertions, deletions);
                            if sim >= threshold {
                                return Some(DamLevMatch {
                                    start: start_pos,
                                    end: end_byte,
                                    insertions,
                                    deletions,
                                    substitutions,
                                    swaps,
                                    similarity: sim,
                                });
                            }
                        }
                    }
                }
            }
        }

        None
    }

    fn find_at_byte_position_large_k(
        &self,
        text: &[u8],
        start_pos: usize,
        threshold: f32,
    ) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        let effective_max = max_edits.min(self.pattern_len);

        let mut r = vec![!0u128; effective_max + 1];
        let mut old_r = vec![!0u128; effective_max + 1];
        let mut old_old_r = vec![!0u128; effective_max + 1]; // State from 2 iterations ago for transposition

        // Initialize deletion states - left shift advances pattern position
        for d in 1..=effective_max {
            r[d] = r[d - 1] << 1;
        }

        // max_window is in characters, but we iterate bytes.
        // For UTF-8, multiply by max char size (4) to ensure we process enough bytes.
        let max_window_chars = self.pattern_len + effective_max;
        let max_window_bytes = if self.is_ascii {
            max_window_chars + 1
        } else {
            max_window_chars * 4 + 1
        };
        let end_limit = (start_pos + max_window_bytes).min(text.len());

        let mut pos = start_pos;
        let mut prev_mask: Option<u128> = None;
        let case_insensitive = self.case_insensitive;
        let mut char_count = 0usize;

        while pos < end_limit && char_count <= max_window_chars {
            let byte = text[pos];

            // Get character mask and length with fast paths
            let (char_mask, char_len) = if byte < 128 {
                let lookup_byte = if case_insensitive {
                    byte.to_ascii_lowercase()
                } else {
                    byte
                };
                (self.byte_masks[lookup_byte as usize], 1)
            } else if byte < 224 && pos + 1 < text.len() {
                // 2-byte UTF-8 fast path
                let b1 = text[pos + 1];
                if case_insensitive {
                    let codepoint = ((u32::from(byte) & 0x1F) << 6) | (u32::from(b1) & 0x3F);
                    let ch = unsafe { char::from_u32_unchecked(codepoint) };
                    let ch_lower = ch.to_lowercase().next().unwrap_or(ch);
                    (self.get_mask(ch_lower), 2)
                } else {
                    (self.get_mask_2byte(byte, b1), 2)
                }
            } else {
                let (ch, len) = decode_utf8_char_fast(text, pos);
                let ch = if case_insensitive {
                    ch.to_lowercase().next().unwrap_or(ch)
                } else {
                    ch
                };
                (self.get_mask(ch), len)
            };

            old_old_r.copy_from_slice(&old_r);
            old_r.copy_from_slice(&r);

            r[0] = (r[0] << 1) | char_mask;

            for d in 1..=effective_max {
                let insert = old_r[d - 1];
                let delete = r[d - 1] << 1; // left shift advances pattern position
                let substitute = old_r[d - 1] << 1;
                let match_d = (old_r[d] << 1) | char_mask;
                let mut new_r = match_d & insert & delete & substitute;

                // Transposition: use cached prev_mask instead of recomputing
                if let Some(pm) = prev_mask {
                    let trans_valid_mask = char_mask | (pm >> 1);
                    // From matched position k, we can reach k+2 via transposition at k+1
                    let trans = ((old_old_r[d - 1] << 1) | trans_valid_mask) << 1;
                    new_r &= trans;
                }

                r[d] = new_r;
            }

            let end_byte = pos + char_len;

            for d in 0..=effective_max {
                if (r[d] & self.accept_mask) == 0 {
                    // Pre-filter: skip error levels where the best possible
                    // similarity is below threshold.
                    let sim_max = 1.0f32 - d as f32 / (self.pattern_len + d) as f32;
                    if sim_max < threshold {
                        continue;
                    }
                    // Compute exact edit breakdown using DP
                    let (insertions, deletions, substitutions, swaps) =
                        self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                    let sim = self.calc_similarity(d as u8, insertions, deletions);
                    if sim >= threshold {
                        return Some(DamLevMatch {
                            start: start_pos,
                            end: end_byte,
                            insertions,
                            deletions,
                            substitutions,
                            swaps,
                            similarity: sim,
                        });
                    }
                }
            }

            prev_mask = Some(char_mask);
            pos += char_len;
            char_count += 1;
        }

        // Handle text shorter than pattern: positions reached during the last iteration
        // need extra match propagation to reach the accept position.
        let end_byte = pos;
        if let Some(last_mask) = prev_mask {
            let chars_short = self.pattern_len.saturating_sub(char_count);
            for _ in 0..chars_short.min(effective_max) {
                old_r.copy_from_slice(&r);

                // Apply match propagation: from position p with d errors,
                // if pattern[p+1] matches last_char, reach position p+1 with d errors
                for d in 1..=effective_max {
                    let match_d = (old_r[d] << 1) | last_mask;
                    r[d] &= match_d;
                }

                // Check for accept after each propagation
                for d in 0..=effective_max {
                    if (r[d] & self.accept_mask) == 0 {
                        let (insertions, deletions, substitutions, swaps) =
                            self.compute_exact_edit_breakdown(&text[start_pos..end_byte]);

                        let total = insertions + deletions + substitutions + swaps;
                        if total as usize <= d {
                            let sim = self.calc_similarity(total, insertions, deletions);
                            if sim >= threshold {
                                return Some(DamLevMatch {
                                    start: start_pos,
                                    end: end_byte,
                                    insertions,
                                    deletions,
                                    substitutions,
                                    swaps,
                                    similarity: sim,
                                });
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Streaming search: scan entire text in one pass, return first match.
    /// This is O(n * k) where n = text length, k = max edits.
    /// Much faster for long texts than repeated `find_at_byte_position` calls.
    #[inline]
    #[must_use]
    pub fn find_first_streaming(&self, text: &[u8], threshold: f32) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;

        // Use const generics for common cases - already highly optimized
        // (only for patterns that fit in u64; longer patterns use the u128 path)
        if self.pattern_len > FAST_PATH_MAX_LEN {
            return self.find_first_streaming_large_k(text, threshold);
        }

        match max_edits {
            0 => self.find_first_streaming_k::<1>(text, threshold, 0),
            1 => self.find_first_streaming_k::<2>(text, threshold, 1),
            2 => self.find_first_streaming_k::<3>(text, threshold, 2),
            3 => self.find_first_streaming_k::<4>(text, threshold, 3),
            4 => self.find_first_streaming_k::<5>(text, threshold, 4),
            _ => self.find_first_streaming_large_k(text, threshold),
        }
    }

    /// Streaming search with const-size state arrays for performance.
    /// Uses three rotating buffers to support transposition detection.
    /// Continues processing after finding fuzzy matches to prefer exact matches.
    #[inline]
    fn find_first_streaming_k<const K: usize>(
        &self,
        text: &[u8],
        threshold: f32,
        max_edits: usize,
    ) -> Option<DamLevMatch> {
        debug_assert!(max_edits < K);

        // Handle empty text: pattern can still match via pure deletions
        if text.is_empty() && self.pattern_len <= max_edits {
            let deletions = self.pattern_len as u8;
            let sim = self.calc_similarity(deletions, 0, deletions);
            if sim >= threshold {
                return Some(DamLevMatch {
                    start: 0,
                    end: 0,
                    insertions: 0,
                    deletions,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
            return None;
        }

        // Use three state arrays for rotation (need old_old for transposition)
        let mut r0 = [!0u128; K];
        let mut r1 = [!0u128; K];
        let mut r2 = [!0u128; K];

        // Initialize: can delete up to max_edits chars from pattern start
        for d in 1..=max_edits {
            r0[d] = r0[d - 1] << 1; // left shift advances pattern position
        }

        // Track byte positions where each error level's current match started
        let mut start_bytes = [0usize; K];

        let byte_masks = &self.byte_masks;
        let accept_mask = self.accept_mask;
        let case_insensitive = self.case_insensitive;

        let mut pos = 0usize;
        let mut rotation = 0usize; // 0, 1, 2 rotation for three buffers
        let mut prev_mask: u128 = !0u128; // Previous character mask for transposition

        // Track best match found so far (prefer fewer edits)
        // After finding a fuzzy match, continue for max_edits more chars to find better matches
        let mut best_match: Option<(usize, DamLevMatch)> = None; // (edit_level, match)
        let mut chars_since_first_match = 0usize;

        while pos < text.len() {
            let byte = text[pos];

            // Get character mask and length (ASCII fast path)
            let (char_mask, char_len) = if byte < 128 {
                let lookup_byte = if case_insensitive {
                    byte.to_ascii_lowercase()
                } else {
                    byte
                };
                (byte_masks[lookup_byte as usize], 1)
            } else if byte < 224 && pos + 1 < text.len() {
                // 2-byte UTF-8 fast path (Cyrillic, Latin Extended, etc.)
                let b1 = text[pos + 1];
                if case_insensitive {
                    // Need full decode for case conversion
                    let codepoint = ((u32::from(byte) & 0x1F) << 6) | (u32::from(b1) & 0x3F);
                    let ch = unsafe { char::from_u32_unchecked(codepoint) };
                    let ch_lower = ch.to_lowercase().next().unwrap_or(ch);
                    (self.get_mask(ch_lower), 2)
                } else {
                    (self.get_mask_2byte(byte, b1), 2)
                }
            } else {
                // 3/4-byte UTF-8 or incomplete sequence
                let (ch, len) = decode_utf8_char_fast(text, pos);
                let ch = if case_insensitive {
                    ch.to_lowercase().next().unwrap_or(ch)
                } else {
                    ch
                };
                (self.get_mask(ch), len)
            };

            // Three-way rotation: old_old -> old -> new
            let (old_old_r, old_r, new_r) = match rotation {
                0 => (&r2, &r0, &mut r1),
                1 => (&r0, &r1, &mut r2),
                _ => (&r1, &r2, &mut r0),
            };

            // Update R[0] (exact matching)
            new_r[0] = (old_r[0] << 1) | char_mask;

            // Update start position for d=0 if no partial match
            if new_r[0] == !0u128 {
                start_bytes[0] = pos + char_len;
            }

            // Update R[d] for d > 0 (fuzzy matching)
            for d in 1..=max_edits {
                let insert = old_r[d - 1]; // consume text char without advancing pattern
                let delete = new_r[d - 1] << 1; // left shift advances pattern position
                let substitute = old_r[d - 1] << 1; // replace pattern char
                let match_d = (old_r[d] << 1) | char_mask;

                let mut new_val = match_d & insert & delete & substitute;

                // Transposition: check if we can swap adjacent chars
                // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
                let trans_valid_mask = char_mask | (prev_mask >> 1);
                // From matched position k, we can reach k+2 via transposition at k+1
                let trans = ((old_old_r[d - 1] << 1) | trans_valid_mask) << 1;
                new_val &= trans;

                new_r[d] = new_val;

                // Update start position if no partial match
                if new_r[d] == !0u128 {
                    start_bytes[d] = pos + char_len;
                }
            }

            // Check for matches (prefer fewer edits)
            let end_byte = pos + char_len;

            for d in 0..=max_edits {
                if (new_r[d] & accept_mask) == 0 {
                    // Streaming found a potential match ending here.
                    // Use tracked start position for this error level (fast path)
                    let tracked_start = start_bytes[d];

                    // Fast path: if tracked start gives exact pattern length match, use it directly
                    if end_byte >= tracked_start {
                        let match_len = end_byte - tracked_start;

                        // Ultra-fast path for exact matches (d=0, length matches exactly)
                        // Skip DP computation entirely - we know it's 0 edits
                        if d == 0 && match_len == self.pattern_len {
                            return Some(DamLevMatch {
                                start: tracked_start,
                                end: end_byte,
                                insertions: 0,
                                deletions: 0,
                                substitutions: 0,
                                swaps: 0,
                                similarity: 1.0,
                            });
                        }

                        // Check if this is likely the best match (close to pattern length)
                        if match_len >= self.pattern_len.saturating_sub(d)
                            && match_len <= self.pattern_len + d
                        {
                            let (insertions, deletions, substitutions, swaps) =
                                self.compute_exact_edit_breakdown(&text[tracked_start..end_byte]);
                            let total = insertions + deletions + substitutions + swaps;

                            if total as usize <= d {
                                let sim = self.calc_similarity(total, insertions, deletions);
                                if sim >= threshold {
                                    // For exact length matches with d=0, return immediately
                                    if d == 0 {
                                        return Some(DamLevMatch {
                                            start: tracked_start,
                                            end: end_byte,
                                            insertions,
                                            deletions,
                                            substitutions,
                                            swaps,
                                            similarity: sim,
                                        });
                                    }

                                    // For fuzzy matches, track as candidate and continue
                                    let candidate = DamLevMatch {
                                        start: tracked_start,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    };

                                    // Prefer: fewer edits, then closer to pattern length
                                    let len_diff =
                                        (match_len as i32 - self.pattern_len as i32).abs();
                                    if best_match.as_ref().is_none_or(|(best_d, b)| {
                                        let b_len = b.end - b.start;
                                        let b_len_diff =
                                            (b_len as i32 - self.pattern_len as i32).abs();
                                        d < *best_d
                                            || (d == *best_d && total < b.total_edits())
                                            || (d == *best_d
                                                && total == b.total_edits()
                                                && len_diff < b_len_diff)
                                    }) {
                                        if best_match.is_none() {
                                            chars_since_first_match = 0;
                                        }
                                        best_match = Some((d, candidate));
                                    }
                                }
                            }
                        }
                    }

                    // For fuzzy matches not caught by fast path, search all possible start positions
                    if d > 0 {
                        let search_start = end_byte.saturating_sub(self.pattern_len + d);

                        for try_start in search_start..end_byte {
                            // Skip if not at a valid UTF-8 char boundary
                            if try_start > 0 && text[try_start] >= 0x80 && text[try_start] < 0xC0 {
                                continue;
                            }

                            // Compute exact edit breakdown using DP
                            let (insertions, deletions, substitutions, swaps) =
                                self.compute_exact_edit_breakdown(&text[try_start..end_byte]);

                            let total = insertions + deletions + substitutions + swaps;
                            if total as usize <= d {
                                let sim = self.calc_similarity(total, insertions, deletions);
                                if sim >= threshold {
                                    let candidate = DamLevMatch {
                                        start: try_start,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    };
                                    // Prefer: fewer edits, then closer to pattern length
                                    let match_len = end_byte - try_start;
                                    let len_diff =
                                        (match_len as i32 - self.pattern_len as i32).abs();
                                    if best_match.as_ref().is_none_or(|(best_d, b)| {
                                        let b_len = b.end - b.start;
                                        let b_len_diff =
                                            (b_len as i32 - self.pattern_len as i32).abs();
                                        d < *best_d
                                            || (d == *best_d && total < b.total_edits())
                                            || (d == *best_d
                                                && total == b.total_edits()
                                                && len_diff < b_len_diff)
                                    }) {
                                        if best_match.is_none() {
                                            chars_since_first_match = 0;
                                        }
                                        best_match = Some((d, candidate));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // After finding a fuzzy match, check if we need to continue looking for better matches.
            // Only continue if the match is "suspicious" (shorter than pattern, indicating possible
            // early accept due to deletions). If match_length >= pattern_length, return immediately.
            if let Some((_, ref m)) = best_match {
                let match_len = m.end - m.start;
                if match_len >= self.pattern_len {
                    // Match is at least pattern length - can't be early accept due to deletions
                    return best_match.map(|(_, m)| m);
                }
                // Short match - might be early accept, continue for a few more chars
                chars_since_first_match += 1;
                if chars_since_first_match > max_edits {
                    return best_match.map(|(_, m)| m);
                }
            }

            prev_mask = char_mask;
            pos += char_len;
            rotation = (rotation + 1) % 3;
        }

        best_match.map(|(_, m)| m)
    }

    /// Streaming search for large k values (uses heap allocation with three-buffer rotation).
    /// Supports transposition detection. Continues processing after fuzzy matches to prefer exact matches.
    fn find_first_streaming_large_k(&self, text: &[u8], threshold: f32) -> Option<DamLevMatch> {
        let max_edits = self.limits.max_edits as usize;
        let effective_max = max_edits.min(self.pattern_len);

        // Handle empty text: pattern can still match via pure deletions
        if text.is_empty() && self.pattern_len <= max_edits {
            let deletions = self.pattern_len as u8;
            let sim = self.calc_similarity(deletions, 0, deletions);
            if sim >= threshold {
                return Some(DamLevMatch {
                    start: 0,
                    end: 0,
                    insertions: 0,
                    deletions,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
            return None;
        }

        // Use three buffers for rotation (need old_old for transposition)
        let mut r0 = vec![!0u128; effective_max + 1];
        let mut r1 = vec![!0u128; effective_max + 1];
        let mut r2 = vec![!0u128; effective_max + 1];
        let mut start_bytes = vec![0usize; effective_max + 1];

        // Initialize: can delete up to effective_max chars from pattern start
        for d in 1..=effective_max {
            r0[d] = r0[d - 1] << 1; // left shift advances pattern position
        }

        let byte_masks = &self.byte_masks;
        let accept_mask = self.accept_mask;
        let case_insensitive = self.case_insensitive;

        let mut pos = 0usize;
        let mut rotation = 0usize; // 0, 1, 2 rotation for three buffers
        let mut prev_mask: u128 = !0u128; // Previous character mask for transposition

        // Track best match found so far (prefer fewer edits)
        // After finding a fuzzy match, continue for effective_max more chars to find better matches
        let mut best_match: Option<(usize, DamLevMatch)> = None; // (edit_level, match)
        let mut chars_since_first_match = 0usize;

        while pos < text.len() {
            let byte = text[pos];

            let (char_mask, char_len) = if byte < 128 {
                let lookup_byte = if case_insensitive {
                    byte.to_ascii_lowercase()
                } else {
                    byte
                };
                (byte_masks[lookup_byte as usize], 1)
            } else if byte < 224 && pos + 1 < text.len() {
                // 2-byte UTF-8 fast path (Cyrillic, Latin Extended, etc.)
                let b1 = text[pos + 1];
                if case_insensitive {
                    let codepoint = ((u32::from(byte) & 0x1F) << 6) | (u32::from(b1) & 0x3F);
                    let ch = unsafe { char::from_u32_unchecked(codepoint) };
                    let ch_lower = ch.to_lowercase().next().unwrap_or(ch);
                    (self.get_mask(ch_lower), 2)
                } else {
                    (self.get_mask_2byte(byte, b1), 2)
                }
            } else {
                // 3/4-byte UTF-8 or incomplete sequence
                let (ch, len) = decode_utf8_char_fast(text, pos);
                let ch = if case_insensitive {
                    ch.to_lowercase().next().unwrap_or(ch)
                } else {
                    ch
                };
                (self.get_mask(ch), len)
            };

            // Three-way rotation: old_old -> old -> new
            let (old_old_r, old_r, new_r) = match rotation {
                0 => (&r2, &r0, &mut r1),
                1 => (&r0, &r1, &mut r2),
                _ => (&r1, &r2, &mut r0),
            };

            // Update R[0] (exact matching)
            new_r[0] = (old_r[0] << 1) | char_mask;
            if new_r[0] == !0u128 {
                start_bytes[0] = pos + char_len;
            }

            // Update R[d] for d > 0 (fuzzy matching)
            for d in 1..=effective_max {
                let insert = old_r[d - 1]; // consume text char without advancing pattern
                let delete = new_r[d - 1] << 1; // left shift advances pattern position
                let substitute = old_r[d - 1] << 1; // replace pattern char
                let match_d = (old_r[d] << 1) | char_mask;

                let mut new_val = match_d & insert & delete & substitute;

                // Transposition: check if we can swap adjacent chars
                // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
                let trans_valid_mask = char_mask | (prev_mask >> 1);
                // From matched position k, we can reach k+2 via transposition at k+1
                let trans = ((old_old_r[d - 1] << 1) | trans_valid_mask) << 1;
                new_val &= trans;

                new_r[d] = new_val;

                if new_r[d] == !0u128 {
                    start_bytes[d] = pos + char_len;
                }
            }

            let end_byte = pos + char_len;

            for d in 0..=effective_max {
                if (new_r[d] & accept_mask) == 0 {
                    // Pre-filter: skip error levels where the best possible
                    // similarity is below threshold.
                    let sim_max = 1.0f32 - d as f32 / (self.pattern_len + d) as f32;
                    if sim_max < threshold {
                        continue;
                    }
                    // Streaming found a potential match ending here.
                    // For exact match (d=0), return immediately
                    if d == 0 {
                        let tracked_start = start_bytes[0];
                        if end_byte >= tracked_start {
                            let match_len = end_byte - tracked_start;
                            if match_len == self.pattern_len {
                                return Some(DamLevMatch {
                                    start: tracked_start,
                                    end: end_byte,
                                    insertions: 0,
                                    deletions: 0,
                                    substitutions: 0,
                                    swaps: 0,
                                    similarity: 1.0,
                                });
                            }
                        }
                    }

                    // Search all possible start positions
                    let search_start = end_byte.saturating_sub(self.pattern_len + d);

                    for try_start in search_start..end_byte {
                        // Skip if not at a valid UTF-8 char boundary
                        if try_start > 0 && text[try_start] >= 0x80 && text[try_start] < 0xC0 {
                            continue;
                        }

                        // Compute exact edit breakdown using DP
                        let (insertions, deletions, substitutions, swaps) =
                            self.compute_exact_edit_breakdown(&text[try_start..end_byte]);

                        let total = insertions + deletions + substitutions + swaps;
                        if total as usize <= d {
                            let sim = self.calc_similarity(total, insertions, deletions);
                            if sim >= threshold {
                                // For exact match (d=0, total=0), return immediately
                                if d == 0 && total == 0 {
                                    return Some(DamLevMatch {
                                        start: try_start,
                                        end: end_byte,
                                        insertions,
                                        deletions,
                                        substitutions,
                                        swaps,
                                        similarity: sim,
                                    });
                                }

                                let candidate = DamLevMatch {
                                    start: try_start,
                                    end: end_byte,
                                    insertions,
                                    deletions,
                                    substitutions,
                                    swaps,
                                    similarity: sim,
                                };
                                // Prefer: fewer edits, then closer to pattern length
                                let match_len = end_byte - try_start;
                                let len_diff = (match_len as i32 - self.pattern_len as i32).abs();
                                if best_match.as_ref().is_none_or(|(best_d, b)| {
                                    let b_len = b.end - b.start;
                                    let b_len_diff = (b_len as i32 - self.pattern_len as i32).abs();
                                    d < *best_d
                                        || (d == *best_d && total < b.total_edits())
                                        || (d == *best_d
                                            && total == b.total_edits()
                                            && len_diff < b_len_diff)
                                }) {
                                    if best_match.is_none() {
                                        chars_since_first_match = 0;
                                    }
                                    best_match = Some((d, candidate));
                                }
                            }
                        }
                    }
                }
            }

            // After finding a fuzzy match, check if we need to continue looking for better matches.
            // Only continue if the match is "suspicious" (shorter than pattern, indicating possible
            // early accept due to deletions). If match_length >= pattern_length, return immediately.
            if let Some((_, ref m)) = best_match {
                let match_len = m.end - m.start;
                if match_len >= self.pattern_len {
                    // Match is at least pattern length - can't be early accept due to deletions
                    return best_match.map(|(_, m)| m);
                }
                // Short match - might be early accept, continue for a few more chars
                chars_since_first_match += 1;
                if chars_since_first_match > effective_max {
                    return best_match.map(|(_, m)| m);
                }
            }

            prev_mask = char_mask;
            pos += char_len;
            rotation = (rotation + 1) % 3;
        }

        best_match.map(|(_, m)| m)
    }
}

// SIMD-accelerated Bitap for ARM with NEON
#[cfg(all(feature = "simd", target_arch = "aarch64"))]
mod simd_neon {
    #[allow(clippy::wildcard_imports)]
    use std::arch::aarch64::*;

    /// NEON state update with transposition for k <= 1.
    #[inline]
    pub unsafe fn update_states_with_trans_k1_neon(
        r: &mut [u64; 4],
        old_r: &[u64; 4],
        old_old_r: &[u64; 4],
        char_mask: u64,
        prev_mask: u64,
    ) {
        unsafe {
            let old_vec = vld1q_u64(old_r.as_ptr());
            let old_old_vec = vld1q_u64(old_old_r.as_ptr());
            let mask = vdupq_n_u64(char_mask);

            // match_d = (old_r[d] << 1) | char_mask
            let match_d = vorrq_u64(vshlq_n_u64(old_vec, 1), mask);

            // insert: [!0, old_r[0]]
            let all_ones = vdupq_n_u64(!0u64);
            let insert = vextq_u64(all_ones, old_vec, 1);

            // substitute = insert << 1
            let subst = vshlq_n_u64(insert, 1);

            // Transposition
            let trans_valid = char_mask | (prev_mask >> 1);
            let trans_valid_vec = vdupq_n_u64(trans_valid);
            let old_old_dm1 = vextq_u64(all_ones, old_old_vec, 1);
            let trans_inner = vorrq_u64(vshlq_n_u64(old_old_dm1, 1), trans_valid_vec);
            let trans = vshlq_n_u64(trans_inner, 1);

            // partial = match_d & insert & substitute & trans
            let partial = vandq_u64(vandq_u64(vandq_u64(match_d, insert), subst), trans);

            vst1q_u64(r.as_mut_ptr(), partial);
            r[1] &= r[0] << 1; // left shift advances pattern position
        }
    }
}

// SIMD-accelerated Bitap for x86_64 with AVX2
#[cfg(all(feature = "simd", target_arch = "x86_64"))]
mod simd_avx2 {
    #[cfg(target_arch = "x86_64")]
    #[allow(clippy::wildcard_imports)]
    use std::arch::x86_64::*;

    /// Check if AVX2 is available at runtime.
    #[inline]
    pub fn is_available() -> bool {
        is_x86_feature_detected!("avx2")
    }

    /// SIMD-accelerated state update for Bitap with k <= 3.
    ///
    /// Computes:
    /// - `r[0]` = (`old_r[0]` << 1) | `char_mask`
    /// - `r[d]` = ((`old_r[d]` << 1) | `char_mask`) & `old_r[d-1]` & (`r[d-1]` << 1) & (`old_r[d-1]` << 1)
    ///
    /// The cascade dependency (r[d] depends on r[d-1]) is handled sequentially after
    /// computing the independent terms in parallel.
    ///
    /// # Safety
    /// Requires AVX2 support. Caller must verify with `is_available()`.
    #[target_feature(enable = "avx2")]
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn update_states_avx2(
        r: &mut [u64; 4],
        old_r: &[u64; 4],
        char_mask: u64,
        max_edits: usize,
    ) {
        debug_assert!(max_edits <= 3);

        unsafe {
            // Load old states into 256-bit register
            let old_vec = _mm256_loadu_si256(old_r.as_ptr().cast::<__m256i>());
            let mask = _mm256_set1_epi64x(char_mask as i64);

            // match_d = (old_r[d] << 1) | char_mask (parallel for all d)
            let match_d = _mm256_or_si256(_mm256_slli_epi64(old_vec, 1), mask);

            // insert = old_r[d-1]: shift lanes right, filling lane 0 with !0
            // Use permute to shift: [old_r[0], old_r[1], old_r[2], old_r[3]] -> [!0, old_r[0], old_r[1], old_r[2]]
            let all_ones = _mm256_set1_epi64x(!0i64);
            // _mm256_permute4x64_epi64 with control 0b10_01_00_11 = [3,0,1,2] but we need [X,0,1,2]
            // Instead, use blend: shift and insert !0 at position 0
            let shifted = _mm256_permute4x64_epi64(old_vec, 0b10_01_00_00); // [0,0,1,2]
            let insert = _mm256_blend_epi32(shifted, all_ones, 0b0000_0011); // lane 0 = !0

            // substitute = old_r[d-1] << 1
            let subst = _mm256_slli_epi64(insert, 1);

            // Combine independent terms: match_d & insert & substitute
            let partial = _mm256_and_si256(_mm256_and_si256(match_d, insert), subst);

            // Store partial results
            _mm256_storeu_si256(r.as_mut_ptr().cast::<__m256i>(), partial);

            // Apply delete cascade: r[d] &= r[d-1] << 1
            // Left shift advances pattern position, must be sequential due to dependency
            if max_edits >= 1 {
                r[1] &= r[0] << 1;
            }
            if max_edits >= 2 {
                r[2] &= r[1] << 1;
            }
            if max_edits >= 3 {
                r[3] &= r[2] << 1;
            }
        }
    }

    /// SIMD-accelerated state update with transposition support.
    ///
    /// # Safety
    /// Requires AVX2 support. Caller must verify with `is_available()`.
    #[target_feature(enable = "avx2")]
    #[inline]
    pub unsafe fn update_states_with_trans_avx2(
        r: &mut [u64; 4],
        old_r: &[u64; 4],
        old_old_r: &[u64; 4],
        char_mask: u64,
        prev_mask: u64,
        max_edits: usize,
    ) {
        debug_assert!(max_edits <= 3);

        unsafe {
            // Load old states
            let old_vec = _mm256_loadu_si256(old_r.as_ptr().cast::<__m256i>());
            let old_old_vec = _mm256_loadu_si256(old_old_r.as_ptr().cast::<__m256i>());
            let mask = _mm256_set1_epi64x(char_mask as i64);

            // match_d = (old_r[d] << 1) | char_mask
            let match_d = _mm256_or_si256(_mm256_slli_epi64(old_vec, 1), mask);

            // Shift for d-1 access
            let all_ones = _mm256_set1_epi64x(!0i64);
            let shifted_old = _mm256_permute4x64_epi64(old_vec, 0b10_01_00_00);
            let insert = _mm256_blend_epi32(shifted_old, all_ones, 0b0000_0011);

            // substitute = old_r[d-1] << 1
            let subst = _mm256_slli_epi64(insert, 1);

            // Transposition term
            // trans_valid_mask: bit j is 0 if pattern[j]=curr AND pattern[j+1]=prev
            let trans_valid = char_mask | (prev_mask >> 1);
            let trans_valid_vec = _mm256_set1_epi64x(trans_valid as i64);

            // Shift old_old for d-1 access
            let shifted_old_old = _mm256_permute4x64_epi64(old_old_vec, 0b10_01_00_00);
            let old_old_dm1 = _mm256_blend_epi32(shifted_old_old, all_ones, 0b0000_0011);

            // trans = ((old_old_r[d-1] << 1) | trans_valid_mask) << 1
            let trans_inner = _mm256_or_si256(_mm256_slli_epi64(old_old_dm1, 1), trans_valid_vec);
            let trans = _mm256_slli_epi64(trans_inner, 1);

            // Combine: match_d & insert & substitute & trans
            let partial = _mm256_and_si256(
                _mm256_and_si256(_mm256_and_si256(match_d, insert), subst),
                trans,
            );

            // Store partial results
            _mm256_storeu_si256(r.as_mut_ptr().cast::<__m256i>(), partial);

            // Apply delete cascade - left shift advances pattern position
            if max_edits >= 1 {
                r[1] &= r[0] << 1;
            }
            if max_edits >= 2 {
                r[2] &= r[1] << 1;
            }
            if max_edits >= 3 {
                r[3] &= r[2] << 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference optimal-string-alignment (restricted Damerau) distance over
    /// chars, matching the breakdown DP's transposition rule.
    fn reference_osa(pattern: &[char], text: &[char]) -> u16 {
        let m = pattern.len();
        let n = text.len();
        let mut d = vec![vec![0u16; n + 1]; m + 1];
        for (i, row) in d.iter_mut().enumerate() {
            row[0] = i as u16;
        }
        for j in 0..=n {
            d[0][j] = j as u16;
        }
        for i in 1..=m {
            for j in 1..=n {
                let cost = u16::from(pattern[i - 1] != text[j - 1]);
                let mut best = (d[i - 1][j] + 1)
                    .min(d[i][j - 1] + 1)
                    .min(d[i - 1][j - 1] + cost);
                if i > 1 && j > 1 && pattern[i - 1] == text[j - 2] && pattern[i - 2] == text[j - 1]
                {
                    best = best.min(d[i - 2][j - 2] + 1);
                }
                d[i][j] = best;
            }
        }
        d[m][n]
    }

    /// The exact breakdown must (a) equal the reference OSA distance and (b) be a
    /// valid alignment (`insertions - deletions == char_len - pattern_len`), for
    /// ASCII and non-ASCII, including over-budget windows. This guards the fixed
    /// early-reject that used to return an invalid `(myers, 0, 0, 0)` tuple. It
    /// also confirms the lightweight distance gate agrees on ASCII.
    #[test]
    fn test_breakdown_is_valid_and_exact() {
        let alpha = ['a', 'b', 'c', 'd', 'é', '中'];
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for ci in [false, true] {
            for _ in 0..20_000 {
                let plen = 1 + (next() % 8) as usize;
                let tlen = (next() % 12) as usize;
                let pat: Vec<char> = (0..plen)
                    .map(|_| alpha[(next() % alpha.len() as u64) as usize])
                    .collect();
                let txt: Vec<char> = (0..tlen)
                    .map(|_| alpha[(next() % alpha.len() as u64) as usize])
                    .collect();
                let pat_s: String = pat.iter().collect();
                let text_s: String = txt.iter().collect();

                // Fold case for the reference when case-insensitive.
                let fold = |v: &[char]| -> Vec<char> {
                    if ci {
                        v.iter().map(|c| c.to_ascii_lowercase()).collect()
                    } else {
                        v.to_vec()
                    }
                };
                let reference = reference_osa(&fold(&pat), &fold(&txt));

                let matcher = BitapMatcher::new(&pat_s, EditLimits::new(4), ci).unwrap();
                let (i, d, s, sw) = matcher.compute_exact_edit_breakdown(text_s.as_bytes());
                let total = u16::from(i) + u16::from(d) + u16::from(s) + u16::from(sw);

                assert_eq!(
                    total, reference,
                    "total != OSA: pat={pat_s:?} text={text_s:?} ci={ci} got {i},{d},{s},{sw}"
                );
                // Valid alignment: net length change matches.
                assert_eq!(
                    i as i32 - d as i32,
                    txt.len() as i32 - pat.len() as i32,
                    "invalid alignment: pat={pat_s:?} text={text_s:?} ci={ci} got {i},{d},{s},{sw}"
                );
                // ASCII distance gate must agree with the breakdown total.
                if let Some(dist) = matcher.compute_damerau_distance(text_s.as_bytes()) {
                    assert_eq!(
                        u16::from(dist),
                        total,
                        "gate != breakdown for {pat_s:?}/{text_s:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_exact_match() {
        let matcher = BitapMatcher::new("hello", EditLimits::new(0), false).unwrap();
        let matches = matcher.find_all("hello world", 0.8);

        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.start == 0 && m.total_edits() == 0));
    }

    #[test]
    fn test_one_substitution() {
        let matcher = BitapMatcher::new("hello", EditLimits::new(1), false).unwrap();
        let matches = matcher.find_all("hallo world", 0.5);

        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.start == 0));
    }

    #[test]
    fn test_find_first() {
        let matcher = BitapMatcher::new("quick", EditLimits::new(1), false).unwrap();
        let result = matcher.find_first("The quick brown fox", 0.8);

        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.start, 4);
    }

    #[test]
    fn test_case_insensitive() {
        let matcher = BitapMatcher::new("hello", EditLimits::new(0), true).unwrap();
        let matches = matcher.find_all("HELLO world", 0.8);

        assert!(!matches.is_empty());
    }

    #[test]
    fn test_pattern_too_long() {
        let long_pattern = "a".repeat(129);
        let result = BitapMatcher::new(&long_pattern, EditLimits::new(1), false);
        assert!(result.is_none());
    }

    #[test]
    fn test_transposition_match() {
        // With transposition support, "ba" should match "ab" with 1 edit (swap)
        // not 2 edits (2 substitutions)
        let matcher = BitapMatcher::new("ab", EditLimits::new(1), false).unwrap();
        let result = matcher.find_at_byte_position(b"ba", 0, 0.5);

        assert!(result.is_some(), "Should find transposition match");
        let m = result.unwrap();
        assert_eq!(m.total_edits(), 1, "Transposition should count as 1 edit");
    }

    #[test]
    fn test_transposition_in_word() {
        // "teh" should match "the" with 1 transposition
        let matcher = BitapMatcher::new("the", EditLimits::new(1), false).unwrap();
        let result = matcher.find_at_byte_position(b"teh quick brown", 0, 0.5);

        assert!(
            result.is_some(),
            "Should find transposition match for 'teh'"
        );
        let m = result.unwrap();
        assert_eq!(m.total_edits(), 1, "Should be 1 edit for transposition");
    }

    #[test]
    fn test_transposition_vs_two_substitutions() {
        // Without transposition, matching "ab" against "ba" would need 2 substitutions
        // With transposition, it's just 1 edit
        // Test that with max_edits=1, we CAN find "ba" because transposition counts as 1
        let matcher = BitapMatcher::new("ab", EditLimits::new(1), false).unwrap();
        let result = matcher.find_at_byte_position(b"ba", 0, 0.0);
        assert!(
            result.is_some(),
            "Transposition should allow match with 1 edit"
        );
    }
}

/// Match result with pattern index for multi-pattern search.
#[derive(Debug, Clone)]
pub struct MultiPatternMatch {
    /// Index of the pattern that matched.
    pub pattern_index: usize,
    /// The match details.
    pub match_result: DamLevMatch,
}

/// A multi-pattern match candidate without the per-operation edit breakdown.
///
/// Produced by [`MultiBitapMatcher::find_all_candidates`]; the breakdown
/// (insertions/deletions/substitutions/swaps) is computed later, only for the
/// matches that are actually returned.
#[derive(Debug, Clone, Copy)]
pub struct MultiPatternCandidate {
    /// Index of the pattern that matched.
    pub pattern_index: usize,
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// Total edit distance (Damerau) of the match.
    pub total_edits: u8,
    /// Similarity score (0.0..=1.0).
    pub similarity: f32,
}

/// Multi-pattern Bitap matcher for searching multiple patterns in a single text pass.
///
/// This is more efficient than running N separate Bitap searches because:
/// 1. Text is scanned only once
/// 2. Character decoding is done once per character
/// 3. Better cache locality
#[derive(Debug)]
pub struct MultiBitapMatcher {
    /// Individual matchers for each pattern.
    matchers: Vec<BitapMatcher>,
    /// Case insensitive matching.
    case_insensitive: bool,
}

impl MultiBitapMatcher {
    /// Create a new multi-pattern matcher.
    ///
    /// Returns None if any pattern is too long or empty.
    #[must_use]
    pub fn new(patterns: &[&str], limits: &EditLimits, case_insensitive: bool) -> Option<Self> {
        if patterns.is_empty() {
            return None;
        }

        let matchers: Option<Vec<BitapMatcher>> = patterns
            .iter()
            .map(|p| BitapMatcher::new(p, limits.clone(), case_insensitive))
            .collect();

        Some(MultiBitapMatcher {
            matchers: matchers?,
            case_insensitive,
        })
    }

    /// Find all match *candidates* for all patterns in a single text pass.
    ///
    /// Returns `(pattern, start, end, total_edits, similarity)` per candidate
    /// WITHOUT the per-operation breakdown. Computing the breakdown (the heavier
    /// 5-tuple DP) is deferred to the caller, which for non-overlapping search
    /// only needs it for the few matches that survive selection. `similarity`
    /// here equals `calc_similarity` exactly (matched length == window length).
    #[must_use]
    pub fn find_all_candidates(&self, text: &str, threshold: f32) -> Vec<MultiPatternCandidate> {
        let text_chars: Vec<(usize, char)> = text.char_indices().collect();

        if text_chars.is_empty() || self.matchers.is_empty() {
            return vec![];
        }

        // Find max_edits across all patterns
        let max_edits = self
            .matchers
            .iter()
            .map(|m| m.limits.max_edits as usize)
            .max()
            .unwrap_or(0);

        // State vectors for each pattern: r[pattern][edit_level]
        let mut r: Vec<Vec<u128>> = self
            .matchers
            .iter()
            .map(|_| vec![!0u128; max_edits + 1])
            .collect();
        let mut old_r: Vec<Vec<u128>> = self
            .matchers
            .iter()
            .map(|_| vec![!0u128; max_edits + 1])
            .collect();

        // Initialize deletion states for each pattern
        for (p_idx, matcher) in self.matchers.iter().enumerate() {
            let p_max = matcher.limits.max_edits as usize;
            for d in 1..=p_max {
                r[p_idx][d] = r[p_idx][d - 1] << 1;
            }
        }

        // Deduplicate candidates by (start, end, pattern); first success wins
        // (the total distance / similarity is deterministic per window).
        let mut candidates: FxHashMap<(usize, usize, usize), MultiPatternCandidate> =
            FxHashMap::default();

        // Process each character once
        for (char_idx, &(_, text_char)) in text_chars.iter().enumerate() {
            let text_char = if self.case_insensitive {
                text_char.to_lowercase().next().unwrap_or(text_char)
            } else {
                text_char
            };

            // Update all pattern states
            for (p_idx, matcher) in self.matchers.iter().enumerate() {
                let p_max = matcher.limits.max_edits as usize;
                let char_mask = matcher.get_mask(text_char);

                // Swap buffers for this pattern
                std::mem::swap(&mut r[p_idx], &mut old_r[p_idx]);

                // Update R[0] (exact matching)
                r[p_idx][0] = (old_r[p_idx][0] << 1) | char_mask;

                // Update R[d] for d > 0 (fuzzy matching)
                for d in 1..=p_max {
                    let insert = old_r[p_idx][d - 1];
                    let delete = r[p_idx][d - 1] << 1;
                    let substitute = old_r[p_idx][d - 1] << 1;
                    let match_d = (old_r[p_idx][d] << 1) | char_mask;
                    r[p_idx][d] = match_d & insert & delete & substitute;
                }

                // Check for matches
                let end_byte = text_chars.get(char_idx + 1).map_or(text.len(), |(b, _)| *b);

                for d in 0..=p_max {
                    if (r[p_idx][d] & matcher.accept_mask) == 0 {
                        // Found a match with d edits for pattern p_idx
                        let min_start_char = char_idx.saturating_sub(matcher.pattern_len + d);
                        let max_start_char =
                            char_idx.saturating_sub(matcher.pattern_len.saturating_sub(d + 1));

                        for start_char in min_start_char..=max_start_char.min(char_idx) {
                            let start_byte = text_chars.get(start_char).map_or(0, |(b, _)| *b);

                            // Cheap pre-DP prune. A window of char-length `L`
                            // matched against a pattern of length `m` needs at
                            // least `|L - m|` edits, so its best-possible
                            // similarity is `1 - |L-m|/max(m,L)`. Skip the
                            // expensive exact-breakdown DP when the window can
                            // neither fit the current edit level nor reach the
                            // similarity threshold. Both are exactly the
                            // conditions checked (and rejected) after the DP.
                            let win_len = char_idx - start_char + 1;
                            let ldiff = win_len.abs_diff(matcher.pattern_len);
                            if ldiff > d {
                                continue;
                            }
                            let max_sim =
                                1.0 - ldiff as f32 / matcher.pattern_len.max(win_len).max(1) as f32;
                            if max_sim < threshold {
                                continue;
                            }

                            let key = (start_byte, end_byte, p_idx);

                            // Already accepted at a lower edit level -> the
                            // (deterministic) result is identical, skip entirely.
                            if candidates.contains_key(&key) {
                                continue;
                            }

                            // Total edit distance via the light distance-only DP
                            // (no per-op breakdown). Falls back to the breakdown's
                            // total for windows too large for the gate (rare).
                            let window = &text.as_bytes()[start_byte..end_byte];
                            let total_edits =
                                matcher.compute_damerau_distance(window).unwrap_or_else(|| {
                                    let (i, d2, s, sw) =
                                        matcher.compute_exact_edit_breakdown(window);
                                    i + d2 + s + sw
                                });
                            if total_edits as usize > d {
                                continue;
                            }
                            // sim == calc_similarity(total, ins, del): matched
                            // length == window length, so it depends only on the
                            // total distance and L.
                            let sim = 1.0
                                - total_edits as f32
                                    / matcher.pattern_len.max(win_len).max(1) as f32;
                            if sim >= threshold {
                                candidates.insert(
                                    key,
                                    MultiPatternCandidate {
                                        pattern_index: p_idx,
                                        start: start_byte,
                                        end: end_byte,
                                        total_edits,
                                        similarity: sim,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        candidates.into_values().collect()
    }

    /// Find all matches for all patterns, with the full per-operation breakdown.
    ///
    /// Thin wrapper over [`find_all_candidates`](Self::find_all_candidates) that
    /// computes the exact edit breakdown for every candidate. Prefer
    /// `find_all_candidates` + a post-selection breakdown when only a subset of
    /// matches is ultimately returned (e.g. non-overlapping search).
    #[must_use]
    pub fn find_all(&self, text: &str, threshold: f32) -> Vec<MultiPatternMatch> {
        self.find_all_candidates(text, threshold)
            .into_iter()
            .map(|c| {
                let (insertions, deletions, substitutions, swaps) = self.matchers[c.pattern_index]
                    .compute_exact_edit_breakdown(&text.as_bytes()[c.start..c.end]);
                MultiPatternMatch {
                    pattern_index: c.pattern_index,
                    match_result: DamLevMatch {
                        start: c.start,
                        end: c.end,
                        insertions,
                        deletions,
                        substitutions,
                        swaps,
                        similarity: c.similarity,
                    },
                }
            })
            .collect()
    }

    /// Compute the exact edit breakdown for a candidate window of a given
    /// pattern. Used to fill in the per-operation counts for the matches that
    /// survive non-overlapping selection (see [`find_all_candidates`](Self::find_all_candidates)).
    #[must_use]
    pub fn breakdown(&self, pattern_index: usize, window: &[u8]) -> (u8, u8, u8, u8) {
        self.matchers[pattern_index].compute_exact_edit_breakdown(window)
    }

    /// Get the number of patterns.
    #[must_use]
    pub fn pattern_count(&self) -> usize {
        self.matchers.len()
    }

    /// Get a pattern by index.
    #[must_use]
    pub fn pattern(&self, index: usize) -> Option<&str> {
        self.matchers.get(index).map(BitapMatcher::pattern)
    }
}
