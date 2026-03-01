//! Optimized NFA-based fuzzy matching.
//!
//! Similar to mrab-regex's fuzzy guards - uses bit-packed state encoding
//! for efficient fuzzy matching without bit-vector algorithms.

#![allow(clippy::needless_range_loop)]

use crate::engine::damlev::{DamLevMatch, EditLimits};

/// Optimized NFA-based fuzzy matcher using bit-packed states.
pub struct FuzzyNfa {
    pattern: Vec<char>,
    pattern_len: usize,
    edit_limits: EditLimits,
    case_insensitive: bool,
    first_char: char,
}

impl FuzzyNfa {
    /// Create a new optimized fuzzy NFA matcher.
    #[must_use]
    pub fn new(pattern: &str, edit_limits: EditLimits, case_insensitive: bool) -> Option<Self> {
        let pattern: Vec<char> = if case_insensitive {
            pattern.to_lowercase().chars().collect()
        } else {
            pattern.chars().collect()
        };
        let pattern_len = pattern.len();

        if pattern_len == 0 || pattern_len > 63 {
            return None;
        }

        let first_char = pattern[0];

        Some(FuzzyNfa {
            pattern,
            pattern_len,
            edit_limits,
            case_insensitive,
            first_char,
        })
    }

    /// Find first match using optimized NFA.
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
                let sim = 1.0 - (self.pattern_len as f32 / (self.pattern_len + max_edits) as f32);
                return Some(DamLevMatch {
                    start: 0,
                    end: 0,
                    insertions: 0,
                    deletions: self.pattern_len as u8,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
            return None;
        }

        let m = self.pattern_len;

        let mut states: [(usize, usize, usize, u8, u8, u8); 128] = [(0, 0, 0, 0, 0, 0); 128];
        let mut state_count = 0;
        let mut seen: u128 = 0;

        let encode_key =
            |pat_pos: usize, edits: usize| -> u128 { ((pat_pos as u128) << 6) | (edits as u128) };

        let mut pos = 0;
        while pos < text_len {
            let text_char = text_chars[pos];
            let mut new_states: [(usize, usize, usize, u8, u8, u8); 128] =
                [(0, 0, 0, 0, 0, 0); 128];
            let mut new_state_count = 0;
            let mut new_seen: u128 = 0;

            let initial_key = encode_key(0, 0);
            if seen & (1u128 << initial_key) == 0 {
                states[0] = (0, 0, pos, 0, 0, 0);
                state_count = 1;
            }

            for i in 0..state_count {
                let (pat_pos, edits, start_pos, ins, del, sub) = states[i];

                if pat_pos >= m || edits > max_edits {
                    continue;
                }

                let pat_char = self.pattern[pat_pos];

                if text_char == pat_char {
                    let key = encode_key(pat_pos + 1, edits);
                    if new_seen & (1u128 << key) == 0 {
                        new_states[new_state_count] =
                            (pat_pos + 1, edits, start_pos, ins, del, sub);
                        new_state_count += 1;
                        new_seen |= 1u128 << key;
                    }
                }

                if edits < max_edits && text_char != pat_char {
                    let key = encode_key(pat_pos + 1, edits + 1);
                    if new_seen & (1u128 << key) == 0 {
                        new_states[new_state_count] =
                            (pat_pos + 1, edits + 1, start_pos, ins, del, sub + 1);
                        new_state_count += 1;
                        new_seen |= 1u128 << key;
                    }
                }

                if edits < max_edits {
                    let key = encode_key(pat_pos, edits + 1);
                    if new_seen & (1u128 << key) == 0 {
                        new_states[new_state_count] =
                            (pat_pos, edits + 1, start_pos, ins + 1, del, sub);
                        new_state_count += 1;
                        new_seen |= 1u128 << key;
                    }
                }

                if pat_pos + 1 < m && edits < max_edits {
                    let key = encode_key(pat_pos + 1, edits + 1);
                    if new_seen & (1u128 << key) == 0 {
                        new_states[new_state_count] =
                            (pat_pos + 1, edits + 1, start_pos, ins, del + 1, sub);
                        new_state_count += 1;
                        new_seen |= 1u128 << key;
                    }
                }
            }

            for i in 0..new_state_count {
                let (pat_pos, edits, start_pos, ins, del, sub) = new_states[i];
                if pat_pos >= m {
                    let sim = 1.0 - (edits as f32 / (m + max_edits) as f32);
                    if sim >= threshold {
                        let match_end = pos + 1;
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

            states = new_states;
            state_count = new_state_count;
            seen = new_seen;

            if state_count == 0 {
                pos += 1;
                while pos < text_len && text_chars[pos] != self.first_char {
                    pos += 1;
                }
            } else {
                pos += 1;
            }
        }

        for i in 0..state_count {
            let (pat_pos, edits, start_pos, ins, del, sub) = states[i];
            if pat_pos >= m {
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
