//! Aho-Corasick fast path for large `\L<name>` word lists.
//!
//! When a pattern is a *pure word-list* reference — a single `\L<name>`
//! (optionally fuzzy) wrapped only in anchors (`^`/`$`) and/or word boundaries
//! (`\b`), with no capture groups — and the resolved list is large, expanding it
//! into an NFA alternation is wasteful (a huge automaton and one bitap scan per
//! word). This module matches such patterns with the
//! [`fuzzy-aho-corasick`](https://github.com/kakserpom/fuzzy-aho-corasick-rs)
//! automaton instead: one pass over the text regardless of list size.
//!
//! Small lists stay on the NFA (it is already correct and fast enough), so this
//! path only changes behavior for genuinely large lists. Selection here is
//! leftmost-longest and non-overlapping, matching the engine's semantics for the
//! equivalent alternation, and the surrounding anchors/boundaries are enforced
//! against the full text so results agree with the NFA.

#![cfg(feature = "word-list-ac")]

use std::borrow::Cow;
use std::cmp::Ordering;

use fuzzy_aho_corasick::{FuzzyAhoCorasick, FuzzyAhoCorasickBuilder, FuzzyLimits};

use crate::engine::EditCounts;

/// A single word-list match in absolute byte offsets.
pub(crate) struct WlMatch {
    pub start: usize,
    pub end: usize,
    pub similarity: f32,
    pub edits: EditCounts,
}

/// Compiled Aho-Corasick fast path for a pure word-list pattern.
pub(crate) struct WordListAc {
    ac: FuzzyAhoCorasick,
    similarity: f32,
    start_anchor: bool,
    end_anchor: bool,
    start_wb: bool,
    end_wb: bool,
}

impl WordListAc {
    /// Build the automaton from the resolved word list.
    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    pub(crate) fn build(
        words: &[Cow<'static, str>],
        edits: u8,
        case_insensitive: bool,
        similarity: f32,
        start_anchor: bool,
        end_anchor: bool,
        start_wb: bool,
        end_wb: bool,
    ) -> Self {
        let mut builder = FuzzyAhoCorasickBuilder::new().case_insensitive(case_insensitive);
        if edits > 0 {
            builder = builder.fuzzy(FuzzyLimits::new().edits(edits));
        }
        let ac = builder.build(words.iter().map(|w| w.as_ref().to_string()));
        WordListAc {
            ac,
            similarity,
            start_anchor,
            end_anchor,
            start_wb,
            end_wb,
        }
    }

    /// Word-boundary test identical to the engine's (`\w` = alphanumeric or `_`).
    fn is_word_boundary(text: &str, pos: usize) -> bool {
        let bytes = text.as_bytes();
        let before_is_word = if pos > 0 {
            let mut start = pos - 1;
            while start > 0 && (bytes[start] & 0xC0) == 0x80 {
                start -= 1;
            }
            text[start..pos]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        } else {
            false
        };
        let after_is_word = text[pos..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        before_is_word != after_is_word
    }

    /// Whether a `[start, end)` span satisfies the pattern's anchors/boundaries.
    fn passes(&self, text: &str, start: usize, end: usize) -> bool {
        if self.start_anchor && start != 0 {
            return false;
        }
        if self.end_anchor && end != text.len() {
            return false;
        }
        if self.start_wb && !Self::is_word_boundary(text, start) {
            return false;
        }
        if self.end_wb && !Self::is_word_boundary(text, end) {
            return false;
        }
        true
    }

    /// All accepted candidate spans (filtered by anchors), sorted leftmost first,
    /// then longest, then highest similarity — the order used for greedy
    /// non-overlapping selection.
    fn candidates(&self, text: &str) -> Vec<WlMatch> {
        let mut cands: Vec<WlMatch> = self
            .ac
            .search(text, self.similarity)
            .iter()
            .filter(|m| m.end > m.start && self.passes(text, m.start, m.end))
            .map(|m| WlMatch {
                start: m.start,
                end: m.end,
                similarity: m.similarity,
                edits: EditCounts {
                    insertions: m.insertions,
                    deletions: m.deletions,
                    substitutions: m.substitutions,
                    swaps: m.swaps,
                },
            })
            .collect();
        cands.sort_by(|a, b| {
            a.start.cmp(&b.start).then(b.end.cmp(&a.end)).then(
                b.similarity
                    .partial_cmp(&a.similarity)
                    .unwrap_or(Ordering::Equal),
            )
        });
        cands
    }

    /// Leftmost match (equivalent to `matches(text).into_iter().next()`).
    pub(crate) fn find(&self, text: &str) -> Option<WlMatch> {
        self.candidates(text).into_iter().next()
    }

    /// Leftmost-longest, non-overlapping matches, left to right.
    pub(crate) fn matches(&self, text: &str) -> Vec<WlMatch> {
        let mut out: Vec<WlMatch> = Vec::new();
        let mut next_start = 0usize;
        for c in self.candidates(text) {
            if c.start >= next_start {
                next_start = c.end;
                out.push(c);
            }
        }
        out
    }

    /// Best match anchored to start exactly at `start` (for `find_at`).
    pub(crate) fn find_at(&self, text: &str, start: usize) -> Option<WlMatch> {
        self.candidates(text).into_iter().find(|m| m.start == start)
    }

    /// All accepted matches (overlapping), leftmost first — for
    /// `captures_all_overlapping`.
    pub(crate) fn all(&self, text: &str) -> Vec<WlMatch> {
        self.candidates(text)
    }
}
