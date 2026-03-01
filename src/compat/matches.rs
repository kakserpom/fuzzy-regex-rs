// Suppress pedantic lints for compatibility layer
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::needless_continue)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::len_without_is_empty)]

//! Match types compatible with fuzzy-aho-corasick.

use super::pattern::Pattern;
use crate::types::NumEdits;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// A single fuzzy match result.
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyMatch<'a> {
    /// Number of insertions.
    pub insertions: NumEdits,
    /// Number of deletions.
    pub deletions: NumEdits,
    /// Number of substitutions.
    pub substitutions: NumEdits,
    /// Number of swaps (transpositions).
    pub swaps: NumEdits,
    /// Total number of edits.
    pub edits: NumEdits,
    /// Index of the matched pattern.
    pub pattern_index: usize,
    /// Reference to the matched pattern.
    pub pattern: &'a Pattern,
    /// Start byte offset in the haystack.
    pub start: usize,
    /// End byte offset in the haystack.
    pub end: usize,
    /// Similarity score (0.0 to 1.0).
    pub similarity: f32,
    /// The matched text slice.
    pub text: &'a str,
}

/// An unmatched segment between fuzzy matches.
#[derive(Debug, Clone, PartialEq)]
pub struct UnmatchedSegment<'a> {
    /// Start byte offset.
    pub start: usize,
    /// End byte offset.
    pub end: usize,
    /// The unmatched text slice.
    pub text: &'a str,
}

/// A segment of text - either matched or unmatched.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment<'a> {
    /// A fuzzy match.
    Matched(FuzzyMatch<'a>),
    /// An unmatched gap.
    Unmatched(UnmatchedSegment<'a>),
}

impl<'a> Segment<'a> {
    /// Get the matched segment if this is a match.
    #[must_use]
    pub fn matched(&'a self) -> Option<&'a FuzzyMatch<'a>> {
        if let Segment::Matched(m) = self {
            Some(m)
        } else {
            None
        }
    }

    /// Get the unmatched segment if this is unmatched.
    #[must_use]
    pub fn unmatched(&'a self) -> Option<&'a UnmatchedSegment<'a>> {
        if let Segment::Unmatched(u) = self {
            Some(u)
        } else {
            None
        }
    }

    /// Get the length of this segment in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Segment::Matched(m) => m.text.len(),
            Segment::Unmatched(u) => u.text.len(),
        }
    }

    /// Check if this segment is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the text content of this segment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Segment::Matched(m) => m.text,
            Segment::Unmatched(u) => u.text,
        }
    }
}

/// Unique ID for pattern deduplication.
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum UniqueId {
    /// Automatic ID based on pattern index.
    Automatic(usize),
    /// Custom user-provided ID.
    Custom(usize),
}

/// A collection of fuzzy matches with sorting and filtering methods.
#[derive(Debug)]
pub struct FuzzyMatches<'a> {
    pub(crate) haystack: &'a str,
    /// The raw list of matches.
    pub inner: Vec<FuzzyMatch<'a>>,
}

impl<'a> FuzzyMatches<'a> {
    /// Default sorting: similarity (desc), pattern length (desc), text length (desc), start (asc).
    pub fn default_sort(&mut self) {
        self.inner.sort_by(|a, b| {
            b.similarity
                .total_cmp(&a.similarity)
                .then_with(|| b.pattern.len().cmp(&a.pattern.len()))
                .then_with(|| b.text.len().cmp(&a.text.len()))
                .then_with(|| a.start.cmp(&b.start))
        });
    }

    /// Greedy sorting: pattern length (desc), similarity (desc), start (asc).
    pub fn greedy_sort(&mut self) {
        self.inner.sort_by(|a, b| {
            b.pattern
                .len()
                .cmp(&a.pattern.len())
                .then_with(|| b.similarity.total_cmp(&a.similarity))
                .then_with(|| a.start.cmp(&b.start))
        });
    }

    /// Coverage-weighted sorting using similarity^2 * `pattern_len`.
    pub fn coverage_weighted_sort(&mut self) {
        self.inner.sort_by(|a, b| {
            let score_a = a.similarity * a.similarity * a.pattern.len() as f32;
            let score_b = b.similarity * b.similarity * b.pattern.len() as f32;
            score_b
                .total_cmp(&score_a)
                .then_with(|| b.similarity.total_cmp(&a.similarity))
                .then_with(|| a.start.cmp(&b.start))
        });
    }

    /// Retain only non-overlapping matches.
    pub fn non_overlapping(&mut self) {
        let mut occupied: BTreeMap<usize, usize> = BTreeMap::new();
        self.inner.retain(|m| {
            let overlaps_before = occupied
                .range(..=m.start)
                .next_back()
                .is_some_and(|(_, &end)| end > m.start);
            let overlaps_after = occupied
                .range(m.start..)
                .next()
                .is_some_and(|(&start, _)| start < m.end);

            if !overlaps_before && !overlaps_after {
                occupied.insert(m.start, m.end);
                true
            } else {
                false
            }
        });
        self.inner.sort_by_key(|m| m.start);
    }

    /// Retain only non-overlapping matches with unique patterns.
    pub fn non_overlapping_unique(&mut self) {
        let mut used_patterns: BTreeSet<UniqueId> = BTreeSet::new();
        let mut occupied: BTreeMap<usize, usize> = BTreeMap::new();

        self.inner.retain(|m| {
            let unique_id = m
                .pattern
                .custom_unique_id
                .map_or(UniqueId::Automatic(m.pattern_index), UniqueId::Custom);

            if used_patterns.contains(&unique_id) {
                return false;
            }

            let overlaps_before = occupied
                .range(..=m.start)
                .next_back()
                .is_some_and(|(_, &end)| end > m.start);
            let overlaps_after = occupied
                .range(m.start..)
                .next()
                .is_some_and(|(&start, _)| start < m.end);

            if !overlaps_before && !overlaps_after {
                used_patterns.insert(unique_id);
                occupied.insert(m.start, m.end);
                true
            } else {
                false
            }
        });
        self.inner.sort_by_key(|m| m.start);
    }

    /// Replace matches using a callback function.
    #[must_use]
    pub fn replace<F, S>(&self, callback: F) -> String
    where
        F: Fn(&FuzzyMatch<'a>) -> Option<S>,
        S: Into<Cow<'a, str>>,
    {
        let mut result = String::new();
        let mut last = 0;

        for m in &self.inner {
            if m.start >= last {
                result.push_str(&self.haystack[last..m.start]);
                last = m.end;

                match callback(m) {
                    Some(repl) => result.push_str(&repl.into()),
                    None => result.push_str(m.text),
                }
            }
        }
        result.push_str(&self.haystack[last..]);
        result
    }

    /// Strip matched prefix and leading whitespace.
    #[must_use]
    pub fn strip_prefix(self) -> String {
        let mut result = String::new();
        let mut skipping = true;

        for segment in self.segment_iter() {
            match segment {
                Segment::Matched(_) if skipping => {}
                Segment::Matched(m) => result.push_str(m.text),
                Segment::Unmatched(u) if skipping => {
                    if u.text.trim().is_empty() {
                        continue;
                    }
                    skipping = false;
                    result.push_str(u.text.trim_start());
                }
                Segment::Unmatched(u) => result.push_str(u.text),
            }
        }
        result
    }

    /// Strip matched suffix and trailing whitespace.
    #[must_use]
    pub fn strip_postfix(self) -> String {
        let segments: Vec<_> = self.segment_iter().collect();
        let mut keep = 0;

        for (i, seg) in segments.iter().enumerate() {
            if let Segment::Unmatched(u) = seg
                && !u.text.trim().is_empty()
            {
                keep = i + 1;
            }
        }

        let mut result = String::new();
        for (i, seg) in segments.into_iter().take(keep).enumerate() {
            let is_last = i + 1 == keep;
            match seg {
                Segment::Matched(m) => result.push_str(m.text),
                Segment::Unmatched(u) if is_last => result.push_str(u.text.trim_end()),
                Segment::Unmatched(u) => result.push_str(u.text),
            }
        }
        result
    }

    /// Split the haystack by matches, returning unmatched parts.
    pub fn split(self) -> impl Iterator<Item = &'a str> + 'a {
        let mut segments = self.segment_iter();
        std::iter::from_fn(move || {
            for seg in segments.by_ref() {
                if let Segment::Unmatched(u) = seg {
                    return Some(u.text);
                }
            }
            None
        })
    }

    /// Iterate over matches.
    pub fn iter(&self) -> impl Iterator<Item = &FuzzyMatch<'a>> {
        self.inner.iter()
    }

    /// Iterate mutably over matches.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut FuzzyMatch<'a>> {
        self.inner.iter_mut()
    }

    /// Get mutable access to the inner vector.
    pub fn inner_mut(&mut self) -> &mut Vec<FuzzyMatch<'a>> {
        &mut self.inner
    }

    /// Get the number of matches.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if there are no matches.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Retain matches that satisfy a predicate.
    pub fn retain<F>(&mut self, pred: F) -> &mut Self
    where
        F: Fn(&FuzzyMatch<'a>) -> bool,
    {
        self.inner.retain(pred);
        self
    }

    /// Filter matches by a predicate, returning a new `FuzzyMatches`.
    #[must_use]
    pub fn filter<F>(&self, pred: F) -> FuzzyMatches<'a>
    where
        F: Fn(&FuzzyMatch<'a>) -> bool,
    {
        FuzzyMatches {
            haystack: self.haystack,
            inner: self.inner.iter().filter(|m| pred(m)).cloned().collect(),
        }
    }

    /// Get the byte spans of all matches.
    #[must_use]
    pub fn matched_spans(&self) -> Vec<(usize, usize)> {
        self.inner.iter().map(|m| (m.start, m.end)).collect()
    }

    /// Get the matched text strings.
    #[must_use]
    pub fn matched_strings(&self) -> Vec<&'a str> {
        self.inner.iter().map(|m| m.text).collect()
    }

    /// Iterate over segments (matched and unmatched).
    pub fn segment_iter(self) -> impl Iterator<Item = Segment<'a>> {
        let mut segments = Vec::new();
        let mut last = 0;

        for m in self.inner {
            if m.start >= last {
                if m.start > last {
                    segments.push(Segment::Unmatched(UnmatchedSegment {
                        start: last,
                        end: m.start,
                        text: &self.haystack[last..m.start],
                    }));
                }
                last = m.end;
                segments.push(Segment::Matched(m));
            }
        }

        if last < self.haystack.len() {
            segments.push(Segment::Unmatched(UnmatchedSegment {
                start: last,
                end: self.haystack.len(),
                text: &self.haystack[last..],
            }));
        }

        segments.into_iter()
    }

    /// Reconstruct text with smart spacing around matches.
    #[must_use]
    pub fn segment_text(self) -> String {
        const SPACE: [char; 2] = [' ', '\t'];
        const NO_LEADING_SPACE: [char; 9] = [',', '.', '?', '!', ';', ':', '—', '-', '…'];

        let mut result = String::new();
        let mut prev_matched = false;

        for segment in self.segment_iter() {
            match segment {
                Segment::Matched(m) => {
                    if prev_matched || (!result.is_empty() && !result.ends_with(SPACE)) {
                        result.push(' ');
                    }
                    prev_matched = true;
                    result.push_str(m.text);
                }
                Segment::Unmatched(u) => {
                    if prev_matched && !u.text.starts_with(NO_LEADING_SPACE) {
                        result.push(' ');
                    }
                    prev_matched = false;
                    result.push_str(u.text);
                }
            }
        }
        result
    }
}

// Iterator implementations
impl<'a, 'b> IntoIterator for &'b FuzzyMatches<'a> {
    type Item = &'b FuzzyMatch<'a>;
    type IntoIter = std::slice::Iter<'b, FuzzyMatch<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a, 'b> IntoIterator for &'b mut FuzzyMatches<'a> {
    type Item = &'b mut FuzzyMatch<'a>;
    type IntoIter = std::slice::IterMut<'b, FuzzyMatch<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

impl<'a> IntoIterator for FuzzyMatches<'a> {
    type Item = FuzzyMatch<'a>;
    type IntoIter = std::vec::IntoIter<FuzzyMatch<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a> std::ops::Deref for FuzzyMatches<'a> {
    type Target = [FuzzyMatch<'a>];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for FuzzyMatches<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
