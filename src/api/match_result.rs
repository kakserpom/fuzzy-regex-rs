//! Match result types for the public API.

#![allow(clippy::needless_range_loop)]

use std::borrow::Cow;
use std::ops::Range;

use smartstring::{Compact, SmartString};

use crate::engine::EditCounts;
use crate::engine::hash::FxHashMap;

/// A single match in the text.
#[derive(Debug, Clone)]
pub struct Match<'t> {
    text: &'t str,
    override_text: Option<SmartString<Compact>>,
    start: usize,
    end: usize,
    similarity: f32,
    edits: EditCounts,
    fuzzy_changes: Option<(Vec<usize>, Vec<usize>, Vec<usize>)>,
    partial: bool,
}

impl<'t> Match<'t> {
    /// Create a new match.
    pub(crate) fn new(
        text: &'t str,
        start: usize,
        end: usize,
        similarity: f32,
        edits: EditCounts,
    ) -> Self {
        Match {
            text,
            override_text: None,
            start,
            end,
            similarity,
            edits,
            fuzzy_changes: None,
            partial: false,
        }
    }

    /// Create a new match with override text.
    pub(crate) fn new_override(
        text: &str,
        start: usize,
        end: usize,
        similarity: f32,
        edits: EditCounts,
    ) -> Self {
        Match {
            text: "",
            override_text: Some(text.into()),
            start,
            end,
            similarity,
            edits,
            fuzzy_changes: None,
            partial: false,
        }
    }

    /// Create a new match with fuzzy changes (edit positions).
    /// Note: This is not currently used because fuzzy changes are computed lazily
    /// via `fuzzy_changes_with_pattern()` for better performance.
    /// Kept for potential future optimization where fuzzy changes are computed during matching.
    #[allow(dead_code)]
    pub(crate) fn new_with_changes(
        text: &'t str,
        start: usize,
        end: usize,
        similarity: f32,
        edits: EditCounts,
        fuzzy_changes: (Vec<usize>, Vec<usize>, Vec<usize>),
    ) -> Self {
        Match {
            text,
            override_text: None,
            start,
            end,
            similarity,
            edits,
            fuzzy_changes: Some(fuzzy_changes),
            partial: false,
        }
    }

    /// Create a new match with all options specified.
    pub(crate) fn new_full(
        text: &'t str,
        start: usize,
        end: usize,
        similarity: f32,
        edits: EditCounts,
        fuzzy_changes: Option<(Vec<usize>, Vec<usize>, Vec<usize>)>,
        partial: bool,
    ) -> Self {
        Match {
            text,
            override_text: None,
            start,
            end,
            similarity,
            edits,
            fuzzy_changes,
            partial,
        }
    }

    /// Get the matched text.
    #[must_use]
    pub fn as_str(&self) -> Cow<'t, str> {
        if let Some(ref r#override) = self.override_text {
            Cow::Owned(r#override.clone().into())
        } else {
            Cow::Borrowed(&self.text[self.start..self.end])
        }
    }

    /// Get the start byte offset.
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Get the end byte offset.
    #[must_use]
    pub fn end(&self) -> usize {
        self.end
    }

    /// Get the byte range.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    /// Get the length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Get the similarity score (0.0 - 1.0).
    #[must_use]
    pub fn similarity(&self) -> f32 {
        self.similarity
    }

    /// Get the edit counts.
    #[must_use]
    pub fn edits(&self) -> &EditCounts {
        &self.edits
    }

    /// Get the total number of edits.
    #[must_use]
    pub fn total_edits(&self) -> u8 {
        self.edits.total()
    }

    /// Get fuzzy counts as (insertions, deletions, substitutions).
    ///
    /// This matches the API of mrab-regex's `fuzzy_counts` property.
    /// - insertions: characters in the text not in the pattern
    /// - deletions: characters in the pattern not in the text
    /// - substitutions: characters that differ between pattern and text
    #[must_use]
    pub fn fuzzy_counts(&self) -> (u32, u32, u32) {
        (
            u32::from(self.edits.insertions),
            u32::from(self.edits.deletions),
            u32::from(self.edits.substitutions),
        )
    }

    /// Get fuzzy changes as (insertions, deletions, substitutions).
    ///
    /// This matches the API of mrab-regex's `fuzzy_changes` property.
    /// Returns positions within the matched text where edits occurred.
    ///
    /// Note: This requires the original pattern to compute accurately.
    /// If the pattern was not provided during matching, returns empty vectors.
    #[must_use]
    pub fn fuzzy_changes(&self) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        if let Some(changes) = &self.fuzzy_changes {
            return changes.clone();
        }

        // If we don't have pre-computed changes, return empty
        // The pattern is needed to compute these accurately
        (Vec::new(), Vec::new(), Vec::new())
    }

    /// Get fuzzy changes given the original pattern.
    ///
    /// This computes the edit positions by comparing the matched text
    /// against the original pattern.
    #[must_use]
    pub fn fuzzy_changes_with_pattern(
        &self,
        pattern: &str,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        let matched_text = self.as_str();
        compute_fuzzy_changes(pattern, matched_text.as_ref())
    }

    /// Check if this is a partial match.
    ///
    /// A partial match is one that reaches the end of the input text
    /// without completing the match. This is useful when searching
    /// through streamed or truncated text.
    #[must_use]
    pub fn partial(&self) -> bool {
        self.partial
    }
}

/// Compute fuzzy changes: positions of insertions, deletions, and substitutions.
/// Returns (insertions, deletions, substitutions) as position vectors.
#[allow(clippy::cast_possible_truncation)]
fn compute_fuzzy_changes(pattern: &str, text: &str) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let a = pattern.as_bytes();
    let b = text.as_bytes();
    let a_len = a.len();
    let b_len = b.len();

    if a_len == 0 {
        let insertions: Vec<usize> = (0..b_len).collect();
        return (insertions, Vec::new(), Vec::new());
    }
    if b_len == 0 {
        let deletions: Vec<usize> = (0..a_len).collect();
        return (Vec::new(), deletions, Vec::new());
    }

    // Build the DP matrix
    let mut matrix = vec![vec![0u32; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i as u32;
    }
    for j in 0..=b_len {
        matrix[0][j] = j as u32;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = u32::from(a[i - 1] != b[j - 1]);
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    // Backtrack to find edit positions
    let mut insertions = Vec::new();
    let mut deletions = Vec::new();
    let mut substitutions = Vec::new();

    let mut i = a_len;
    let mut j = b_len;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            i -= 1;
            j -= 1;
        } else if i > 0 && j > 0 && matrix[i][j] == matrix[i - 1][j - 1] + 1 {
            substitutions.push(j - 1);
            i -= 1;
            j -= 1;
        } else if j > 0 && matrix[i][j] == matrix[i][j - 1] + 1 {
            insertions.push(j - 1);
            j -= 1;
        } else if i > 0 && matrix[i][j] == matrix[i - 1][j] + 1 {
            deletions.push(i - 1);
            i -= 1;
        } else {
            break;
        }
    }

    (insertions, deletions, substitutions)
}

/// Capture groups from a match.
#[derive(Debug, Clone)]
pub struct Captures<'t> {
    text: &'t str,
    slots: Vec<Option<(usize, usize)>>,
    names: FxHashMap<SmartString<Compact>, usize>,
    similarity: f32,
    edits: EditCounts,
    handler_overrides: Vec<(usize, usize, SmartString<Compact>)>,
}

impl<'t> Captures<'t> {
    /// Create captures from match result.
    pub(crate) fn new(
        text: &'t str,
        slots: Vec<Option<(usize, usize)>>,
        names: FxHashMap<SmartString<Compact>, usize>,
        similarity: f32,
        edits: EditCounts,
        handler_overrides: Vec<(usize, usize, SmartString<Compact>)>,
    ) -> Self {
        Captures {
            text,
            slots,
            names,
            similarity,
            edits,
            handler_overrides,
        }
    }

    /// Apply handler overrides to capture text.
    fn apply_overrides(&self, start: usize, end: usize) -> SmartString<Compact> {
        if self.handler_overrides.is_empty() {
            return self.text[start..end].into();
        }

        let mut result = SmartString::new();
        let mut pos = start;

        for (ov_start, ov_end, ov_text) in &self.handler_overrides {
            let ov_start = *ov_start;
            let ov_end = *ov_end;

            if ov_end <= start || ov_start >= end {
                continue;
            }

            if ov_start > pos {
                result.push_str(&self.text[pos..ov_start.min(end)]);
            }

            let overlap_start = ov_start.max(start);
            let overlap_end = ov_end.min(end);
            if overlap_start < overlap_end {
                result.push_str(ov_text);
            }

            pos = ov_end;
        }

        if pos < end {
            result.push_str(&self.text[pos..end]);
        }

        result
    }

    /// Get the full match (group 0).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Match<'t>> {
        self.slots
            .get(index)
            .copied()
            .flatten()
            .map(|(start, end)| {
                if self.handler_overrides.is_empty() {
                    Match::new(self.text, start, end, self.similarity, self.edits.clone())
                } else {
                    let text = self.apply_overrides(start, end);
                    Match::new_override(&text, start, end, self.similarity, self.edits.clone())
                }
            })
    }

    /// Get a capture by name.
    #[must_use]
    pub fn name(&self, name: &str) -> Option<Match<'t>> {
        self.names.get(name).and_then(|&idx| self.get(idx))
    }

    /// Get the number of capture groups (including group 0).
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Check if there are no captures.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Get handler overrides.
    #[must_use]
    pub fn handler_overrides(&self) -> &[(usize, usize, SmartString<Compact>)] {
        &self.handler_overrides
    }

    /// Iterate over all captures.
    #[must_use]
    pub fn iter(&self) -> CapturesIter<'_, 't> {
        CapturesIter {
            captures: self,
            index: 0,
        }
    }

    /// Get the similarity score.
    #[must_use]
    pub fn similarity(&self) -> f32 {
        self.similarity
    }

    /// Get edit counts.
    #[must_use]
    pub fn edits(&self) -> &EditCounts {
        &self.edits
    }

    /// Get fuzzy counts as (insertions, deletions, substitutions).
    ///
    /// This matches the API of mrab-regex's `fuzzy_counts` property.
    #[must_use]
    pub fn fuzzy_counts(&self) -> (u32, u32, u32) {
        (
            u32::from(self.edits.insertions),
            u32::from(self.edits.deletions),
            u32::from(self.edits.substitutions),
        )
    }

    /// Get fuzzy changes as (insertions, deletions, substitutions).
    ///
    /// This matches the API of mrab-regex's `fuzzy_changes` property.
    #[must_use]
    pub fn fuzzy_changes(&self) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        (Vec::new(), Vec::new(), Vec::new())
    }

    /// Expand a replacement string with capture references.
    ///
    /// Supports `$0`, `$1`, etc. for numbered groups and `$name` for named groups.
    #[must_use]
    pub fn expand(&self, replacement: &str) -> String {
        let mut result = String::new();
        let mut chars = replacement.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                match chars.peek() {
                    Some('$') => {
                        chars.next();
                        result.push('$');
                    }
                    Some(&c) if c.is_ascii_digit() => {
                        let mut num = 0usize;
                        while let Some(&c) = chars.peek() {
                            if let Some(d) = c.to_digit(10) {
                                num = num * 10 + d as usize;
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Some(m) = self.get(num) {
                            result.push_str(&m.as_str());
                        }
                    }
                    Some(&c) if c.is_alphabetic() || c == '_' => {
                        let mut name = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_alphanumeric() || c == '_' {
                                name.push(c);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Some(m) = self.name(&name) {
                            result.push_str(&m.as_str());
                        }
                    }
                    Some('{') => {
                        chars.next(); // consume '{'
                        let mut name = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == '}' {
                                chars.next();
                                break;
                            }
                            name.push(c);
                            chars.next();
                        }
                        // Try as number first
                        if let Ok(num) = name.parse::<usize>() {
                            if let Some(m) = self.get(num) {
                                result.push_str(&m.as_str());
                            }
                        } else if let Some(m) = self.name(&name) {
                            result.push_str(&m.as_str());
                        }
                    }
                    _ => result.push('$'),
                }
            } else {
                result.push(ch);
            }
        }

        result
    }
}

/// Iterator over captures.
pub struct CapturesIter<'c, 't> {
    captures: &'c Captures<'t>,
    index: usize,
}

impl<'t> Iterator for CapturesIter<'_, 't> {
    type Item = Option<Match<'t>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.captures.slots.len() {
            let result = self.captures.get(self.index);
            self.index += 1;
            Some(result)
        } else {
            None
        }
    }
}

impl<'c, 't> IntoIterator for &'c Captures<'t> {
    type Item = Option<Match<'t>>;
    type IntoIter = CapturesIter<'c, 't>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over all matches.
pub struct Matches<'t> {
    matches: std::vec::IntoIter<Match<'t>>,
}

impl<'t> Matches<'t> {
    /// Create a Matches iterator from pre-collected matches.
    pub(crate) fn new(matches: Vec<Match<'t>>) -> Self {
        Matches {
            matches: matches.into_iter(),
        }
    }
}

impl<'t> Iterator for Matches<'t> {
    type Item = Match<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        self.matches.next()
    }
}

/// Iterator over all capture groups.
pub struct CaptureMatches<'r, 't> {
    pub(crate) regex: &'r super::regex::FuzzyRegex,
    pub(crate) text: &'t str,
    pub(crate) pos: usize,
}

impl<'t> Iterator for CaptureMatches<'_, 't> {
    type Item = Captures<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos > self.text.len() {
            return None;
        }

        let result = self.regex.captures_at(self.text, self.pos);

        if let Some(caps) = result {
            if let Some(m) = caps.get(0) {
                self.pos = if m.end() > self.pos {
                    m.end()
                } else {
                    self.text[self.pos..]
                        .char_indices()
                        .nth(1)
                        .map_or(self.text.len() + 1, |(i, _)| self.pos + i)
                };
            } else {
                self.pos = self.text.len() + 1;
            }
            Some(caps)
        } else {
            self.pos = self.text.len() + 1;
            None
        }
    }
}

/// Iterator over split segments.
pub struct Split<'r, 't> {
    pub(crate) regex: &'r super::regex::FuzzyRegex,
    pub(crate) text: &'t str,
    pub(crate) pos: usize,
    pub(crate) done: bool,
}

impl<'t> Iterator for Split<'_, 't> {
    type Item = &'t str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        if self.pos > self.text.len() {
            self.done = true;
            return None;
        }

        // Use find_from to search from current position onwards
        let result = self.regex.find_from(self.text, self.pos);

        if let Some(m) = result {
            let segment = &self.text[self.pos..m.start()];
            self.pos = m.end();
            Some(segment)
        } else {
            let segment = &self.text[self.pos..];
            self.done = true;
            Some(segment)
        }
    }
}
