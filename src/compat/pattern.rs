//! Pattern struct compatible with fuzzy-aho-corasick.

use crate::types::{FuzzyLimits, NumEdits};

/// A search pattern with optional weight and fuzzy limits.
///
/// Patterns can be created from strings or tuples for convenience.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// The pattern string.
    pub pattern: String,
    /// Length in grapheme clusters.
    pub grapheme_len: usize,
    /// Optional custom ID for uniqueness tracking.
    pub custom_unique_id: Option<usize>,
    /// Pattern weight (default 1.0).
    pub weight: f32,
    /// Per-pattern fuzzy limits.
    pub limits: Option<FuzzyLimits>,
}

impl Pattern {
    /// Get the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    /// Get the byte length of the pattern.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pattern.len()
    }

    /// Check if the pattern is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pattern.is_empty()
    }

    /// Set pattern weight.
    #[must_use]
    pub fn weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    /// Set per-pattern fuzzy limits.
    #[must_use]
    pub fn fuzzy(mut self, limits: FuzzyLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Set custom unique ID for pattern deduplication.
    #[must_use]
    pub fn custom_unique_id(mut self, id: usize) -> Self {
        self.custom_unique_id = Some(id);
        self
    }
}

fn count_graphemes(s: &str) -> usize {
    unicode_segmentation::UnicodeSegmentation::graphemes(s, true).count()
}

impl From<&str> for Pattern {
    fn from(s: &str) -> Self {
        Pattern {
            pattern: s.to_owned(),
            grapheme_len: count_graphemes(s),
            weight: 1.0,
            limits: None,
            custom_unique_id: None,
        }
    }
}

impl From<String> for Pattern {
    fn from(s: String) -> Self {
        let grapheme_len = count_graphemes(&s);
        Pattern {
            pattern: s,
            grapheme_len,
            weight: 1.0,
            limits: None,
            custom_unique_id: None,
        }
    }
}

impl From<&String> for Pattern {
    fn from(s: &String) -> Self {
        Pattern {
            pattern: s.clone(),
            grapheme_len: count_graphemes(s),
            weight: 1.0,
            limits: None,
            custom_unique_id: None,
        }
    }
}

impl From<(&str, f32)> for Pattern {
    fn from((s, w): (&str, f32)) -> Self {
        Pattern {
            pattern: s.to_owned(),
            grapheme_len: count_graphemes(s),
            weight: w,
            limits: None,
            custom_unique_id: None,
        }
    }
}

impl From<(String, f32)> for Pattern {
    fn from((s, w): (String, f32)) -> Self {
        let grapheme_len = count_graphemes(&s);
        Pattern {
            pattern: s,
            grapheme_len,
            weight: w,
            limits: None,
            custom_unique_id: None,
        }
    }
}

impl From<(&String, f32)> for Pattern {
    fn from((s, w): (&String, f32)) -> Self {
        Pattern {
            pattern: s.clone(),
            grapheme_len: count_graphemes(s),
            weight: w,
            limits: None,
            custom_unique_id: None,
        }
    }
}

impl From<(&str, f32, NumEdits)> for Pattern {
    fn from((s, w, max_edits): (&str, f32, NumEdits)) -> Self {
        Pattern {
            pattern: s.to_owned(),
            grapheme_len: count_graphemes(s),
            weight: w,
            limits: Some(FuzzyLimits::new().edits(max_edits)),
            custom_unique_id: None,
        }
    }
}

impl From<(String, f32, NumEdits)> for Pattern {
    fn from((s, w, max_edits): (String, f32, NumEdits)) -> Self {
        let grapheme_len = count_graphemes(&s);
        Pattern {
            pattern: s,
            grapheme_len,
            weight: w,
            limits: Some(FuzzyLimits::new().edits(max_edits)),
            custom_unique_id: None,
        }
    }
}
