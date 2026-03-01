//! Myers' bit-vector algorithm for approximate string matching.
//!
//! Reference: "A fast bit-vector algorithm for approximate string matching
//! based on dynamic programming" by Gene Myers (1999/JACM).

#![allow(clippy::needless_range_loop)]

use super::damlev::{DamLevMatch, EditLimits};

/// Myers bit-vector matcher - uses bit-parallel DP for O(n) performance.
#[derive(Debug)]
pub struct MyersMatcher {
    pattern_len: usize,
    edit_limits: EditLimits,
    #[allow(dead_code)]
    pub masks: Vec<u64>,
    #[allow(dead_code)]
    accept_mask: u64,
}

impl MyersMatcher {
    /// Create a new Myers matcher.
    #[must_use]
    pub fn new(pattern: &str, edit_limits: EditLimits, case_insensitive: bool) -> Option<Self> {
        if case_insensitive {
            return None;
        }

        let pattern_bytes = pattern.as_bytes();
        let m = pattern_bytes.len();

        if m == 0 || m > 63 {
            return None;
        }

        // Build character masks
        let mut masks = vec![0u64; 256];
        for (i, &byte) in pattern_bytes.iter().enumerate() {
            masks[byte as usize] |= 1u64 << i;
        }

        Some(MyersMatcher {
            pattern_len: m,
            edit_limits,
            masks,
            accept_mask: 1u64 << (m - 1),
        })
    }

    /// Calculate edit distance from current state.
    #[allow(dead_code)]
    #[inline]
    #[allow(clippy::unused_self)]
    fn get_score(&self, vp: u64, vn: u64) -> usize {
        let score_mask = !vp & vn;
        if score_mask == 0 {
            return 0;
        }
        score_mask.trailing_zeros() as usize
    }

    /// Find first match using Myers' algorithm.
    /// Uses dynamic programming to track minimum score at each position.
    #[inline]
    #[must_use]
    pub fn find_first(&self, text: &str, threshold: f32) -> Option<DamLevMatch> {
        let max_edits = self.edit_limits.max_edits as usize;
        let m = self.pattern_len;

        if m == 0 {
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

        let text = text.as_bytes();
        let n = text.len();

        if n == 0 {
            if m <= max_edits {
                let sim = 1.0 - (m as f32 / (m + max_edits) as f32);
                return Some(DamLevMatch {
                    start: 0,
                    end: 0,
                    insertions: 0,
                    deletions: m as u8,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
            return None;
        }

        // Track minimum score at each position
        let mut min_score = m + 1;
        let mut min_pos = 0;

        // Run Myers through text
        let mut vp: u64 = !0u64;
        let mut vn: u64 = 0u64;
        let mut score = m;

        for i in 0..n {
            let eq = self.masks[text[i] as usize];

            let x = eq | vn;
            let y = (vp & x).wrapping_add(vp);
            let d0 = (y ^ vp) | x;

            let hp = vn | !d0;
            let hn = vp & d0;

            if hp & (1u64 << (m - 1)) != 0 {
                score = score.saturating_add(1);
            }
            if hn & (1u64 << (m - 1)) != 0 {
                score = score.saturating_sub(1);
            }

            let hp_shift = (hp << 1) | 1;
            let vn_shift = hn << 1;

            vp = vn_shift | !(x | hp_shift);
            vn = hp_shift & x;

            // After at least m chars processed, check for match
            // The match could end at position i, starting at i-m+1
            if i >= m - 1 && score <= max_edits && score < min_score {
                min_score = score;
                min_pos = i + 1;
            }
        }

        if min_score <= max_edits {
            let sim = 1.0 - (min_score as f32 / (m + max_edits) as f32);
            if sim >= threshold {
                let start = min_pos.saturating_sub(m);
                return Some(DamLevMatch {
                    start,
                    end: min_pos,
                    insertions: 0,
                    deletions: min_score as u8,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
        }

        None
    }

    /// Find all matches.
    #[must_use]
    pub fn find_all(&self, text: &str, threshold: f32) -> Vec<DamLevMatch> {
        let max_edits = self.edit_limits.max_edits as usize;
        let m = self.pattern_len;
        let mut matches = Vec::new();

        if m == 0 {
            return vec![DamLevMatch {
                start: 0,
                end: 0,
                insertions: 0,
                deletions: 0,
                substitutions: 0,
                swaps: 0,
                similarity: 1.0,
            }];
        }

        let text = text.as_bytes();
        let n = text.len();

        if n == 0 {
            if m <= max_edits {
                let sim = 1.0 - (m as f32 / (m + max_edits) as f32);
                matches.push(DamLevMatch {
                    start: 0,
                    end: 0,
                    insertions: 0,
                    deletions: m as u8,
                    substitutions: 0,
                    swaps: 0,
                    similarity: sim,
                });
            }
            return matches;
        }

        let mut vp: u64 = !0u64;
        let mut vn: u64 = 0u64;
        let mut score = m;

        // For find_all, we need to track matches at each position
        // since the minimum score could be at any position
        for i in 0..n {
            let eq = self.masks[text[i] as usize];

            let x = eq | vn;
            let y = (vp & x).wrapping_add(vp);
            let d0 = (y ^ vp) | x;

            let hp = vn | !d0;
            let hn = vp & d0;

            if hp & (1u64 << (m - 1)) != 0 {
                score = score.saturating_add(1);
            }
            if hn & (1u64 << (m - 1)) != 0 {
                score = score.saturating_sub(1);
            }

            let hp_shift = (hp << 1) | 1;
            let vn_shift = hn << 1;

            vp = vn_shift | !(x | hp_shift);
            vn = hp_shift & x;

            // Check for match after processing at least m characters
            if i >= m - 1 && score <= max_edits {
                let sim = 1.0 - (score as f32 / (m + max_edits) as f32);

                if sim >= threshold {
                    let end = i + 1;
                    let start = end.saturating_sub(m);

                    matches.push(DamLevMatch {
                        start,
                        end,
                        insertions: 0,
                        deletions: score as u8,
                        substitutions: 0,
                        swaps: 0,
                        similarity: sim,
                    });
                }
            }
        }

        matches
    }
}
