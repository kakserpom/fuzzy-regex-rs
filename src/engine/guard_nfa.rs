//! Optimized Guard-based Levenshtein NFA implementation.
//!
//! Similar to mrab-regex's fuzzy guards - uses bit-packed state encoding
//! for efficient fuzzy matching with early termination.

#![allow(
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::allow_attributes,
    let_underscore_drop
)]

use crate::engine::damlev::{DamLevMatch, EditLimits};

/// Guard-based NFA for fast fuzzy matching.
#[derive(Debug)]
pub struct GuardNfa {
    pattern: Vec<char>,
    pattern_len: usize,
    edit_limits: EditLimits,
    case_insensitive: bool,
    first_char: char,
}

impl GuardNfa {
    /// Create a new guard-based NFA.
    #[must_use]
    pub fn new(pattern: &str, edit_limits: EditLimits, case_insensitive: bool) -> Self {
        let pattern: Vec<char> = if case_insensitive {
            pattern.to_lowercase().chars().collect()
        } else {
            pattern.chars().collect()
        };
        let pattern_len = pattern.len();
        let first_char = pattern.first().copied().unwrap_or('\0');

        GuardNfa {
            pattern,
            pattern_len,
            edit_limits,
            case_insensitive,
            first_char,
        }
    }

    /// Find the first match in text with early termination.
    #[inline]
    #[must_use]
    pub fn find_first(&self, text: &str, threshold: f32) -> Option<DamLevMatch> {
        let max_edits = self.edit_limits.max_edits as usize;

        if self.pattern_len == 0 {
            return Some(DamLevMatch {
                start: 0,
                end: 0,
                insertions: 0,
                deletions: 0,
                substitutions: 0,
                swaps: 0,
                similarity: 1.0,
            });
        }

        let text_chars: Vec<char> = if self.case_insensitive {
            text.chars()
                .map(|c| c.to_lowercase().next().unwrap_or(c))
                .collect()
        } else {
            text.chars().collect()
        };
        let text_len = text_chars.len();

        let mut char_to_byte: Vec<usize> = vec![0; text_len + 1];
        let mut byte_pos = 0;
        for (i, c) in text.char_indices() {
            char_to_byte[i] = byte_pos;
            byte_pos += c.len_utf8();
        }
        char_to_byte[text_len] = byte_pos;

        if text_len == 0 {
            if self.pattern_len <= max_edits {
                let edits = self.pattern_len;
                let sim = 1.0 - (edits as f32 / (self.pattern_len + max_edits) as f32);
                return Some(DamLevMatch {
                    start: 0,
                    end: 0,
                    insertions: 0,
                    deletions: edits as u8,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
            return None;
        }

        let m = self.pattern_len;

        // Vec-based states but with bit-packed seen for O(1) deduplication
        let mut active: Vec<(usize, usize, usize, u8, u8, u8, usize)> = Vec::with_capacity(32);
        let mut next_active: Vec<(usize, usize, usize, u8, u8, u8, usize)> = Vec::with_capacity(32);

        // Bit-packed seen: 12 bits (6 for pat_pos + 6 for edits)
        let encode_key =
            |pat_pos: usize, edits: usize| -> u128 { ((pat_pos as u128) << 6) | (edits as u128) };

        let mut pos = 0;
        while pos < text_len {
            let text_char = text_chars[pos];
            let mut new_seen: u128 = 0;

            // Start new match at this position
            active.clear();
            active.push((0, 0, pos, 0, 0, 0, pos));

            next_active.clear();

            // Process all active states
            for &(pat_pos, edits, start_pos, ins, del, sub, last_consumed) in &active {
                if pat_pos >= m || edits > max_edits {
                    continue;
                }

                let pat_char = self.pattern[pat_pos];

                // Exact match
                if text_char == pat_char {
                    let key = encode_key(pat_pos + 1, edits);
                    if new_seen & (1u128 << key) == 0 {
                        new_seen |= 1u128 << key;
                        next_active.push((pat_pos + 1, edits, start_pos, ins, del, sub, pos));
                    }
                }

                // Substitution
                if edits < max_edits && text_char != pat_char {
                    let key = encode_key(pat_pos + 1, edits + 1);
                    if new_seen & (1u128 << key) == 0 {
                        new_seen |= 1u128 << key;
                        next_active.push((
                            pat_pos + 1,
                            edits + 1,
                            start_pos,
                            ins,
                            del,
                            sub + 1,
                            pos,
                        ));
                    }
                }

                // Insertion
                if edits < max_edits {
                    let key = encode_key(pat_pos, edits + 1);
                    if new_seen & (1u128 << key) == 0 {
                        new_seen |= 1u128 << key;
                        next_active.push((
                            pat_pos,
                            edits + 1,
                            start_pos,
                            ins + 1,
                            del,
                            sub,
                            last_consumed,
                        ));
                    }
                }

                // Deletion
                if pat_pos + 1 < m && edits < max_edits {
                    let key = encode_key(pat_pos + 1, edits + 1);
                    if new_seen & (1u128 << key) == 0 {
                        new_seen |= 1u128 << key;
                        next_active.push((
                            pat_pos + 1,
                            edits + 1,
                            start_pos,
                            ins,
                            del + 1,
                            sub,
                            last_consumed,
                        ));
                    }
                }
            }

            // Check for matches - return immediately on first match
            for &(pat_pos, edits, start_pos, ins, del, sub, last_consumed) in &next_active {
                if pat_pos >= m && edits <= max_edits {
                    let sim = 1.0 - (edits as f32 / (m + max_edits) as f32);
                    if sim >= threshold {
                        let match_end = last_consumed + 1;
                        let byte_start = char_to_byte[start_pos];
                        let byte_end = char_to_byte[match_end];
                        return Some(DamLevMatch {
                            start: byte_start,
                            end: byte_end,
                            insertions: ins,
                            deletions: del,
                            substitutions: sub,
                            swaps: 0,
                            similarity: sim,
                        });
                    }
                }
            }

            std::mem::swap(&mut active, &mut next_active);

            // Guard pruning - skip to first character if no active states
            if active.is_empty() {
                pos += 1;
                while pos < text_len && text_chars[pos] != self.first_char {
                    pos += 1;
                }
            } else {
                pos += 1;
            }
        }

        // Handle trailing deletions
        for &(pat_pos, edits, start_pos, ins, del, sub, _last_consumed) in &active {
            if pat_pos >= m && edits <= max_edits {
                let sim = 1.0 - (edits as f32 / (m + max_edits) as f32);
                if sim >= threshold {
                    let byte_start = char_to_byte[start_pos];
                    let byte_end = char_to_byte[text_len];
                    return Some(DamLevMatch {
                        start: byte_start,
                        end: byte_end,
                        insertions: ins,
                        deletions: del,
                        substitutions: sub,
                        swaps: 0,
                        similarity: sim,
                    });
                }
            }

            let remaining = m - pat_pos;
            let new_edits = edits + remaining;
            if new_edits <= max_edits {
                let new_del = del + remaining as u8;
                let sim = 1.0 - (new_edits as f32 / (m + max_edits) as f32);
                if sim >= threshold {
                    let byte_start = char_to_byte[start_pos];
                    let byte_end = char_to_byte[text_len];
                    return Some(DamLevMatch {
                        start: byte_start,
                        end: byte_end,
                        insertions: ins,
                        deletions: new_del,
                        substitutions: sub,
                        swaps: 0,
                        similarity: sim,
                    });
                }
            }
        }

        None
    }
}
