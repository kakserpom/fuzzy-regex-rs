//! Core types for fuzzy matching limits and penalties.
pub type NumEdits = u8;
pub type Distance = u16;

/// Limits on the number of edit operations allowed during fuzzy matching.
///
/// Edit operations include:
/// - **Insertions**: Extra characters in the text that aren't in the pattern
/// - **Deletions**: Characters in the pattern that are missing from the text
/// - **Substitutions**: Characters that differ between pattern and text
/// - **Swaps**: Adjacent character transpositions
///
/// You can set a total edit limit with `edits()`, or individual limits for each
/// operation type. When individual limits are set without a total, the total is
/// computed as the sum of individual limits.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FuzzyLimits {
    insertions: Option<NumEdits>,
    deletions: Option<NumEdits>,
    substitutions: Option<NumEdits>,
    swaps: Option<NumEdits>,
    edits: Option<NumEdits>,
}

impl FuzzyLimits {
    /// Create new empty fuzzy limits (exact match only).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum number of insertions allowed.
    #[must_use]
    pub fn insertions(mut self, num: NumEdits) -> Self {
        self.insertions = Some(num);
        self
    }

    /// Set the maximum number of deletions allowed.
    #[must_use]
    pub fn deletions(mut self, num: NumEdits) -> Self {
        self.deletions = Some(num);
        self
    }

    /// Set the maximum number of substitutions allowed.
    #[must_use]
    pub fn substitutions(mut self, num: NumEdits) -> Self {
        self.substitutions = Some(num);
        self
    }

    /// Set the maximum number of swaps (transpositions) allowed.
    #[must_use]
    pub fn swaps(mut self, num: NumEdits) -> Self {
        self.swaps = Some(num);
        self
    }

    /// Set the maximum total number of edits allowed.
    #[must_use]
    pub fn edits(mut self, num: NumEdits) -> Self {
        self.edits = Some(num);
        self
    }

    /// Get the maximum total edits allowed.
    #[must_use]
    pub fn get_edits(&self) -> Option<NumEdits> {
        self.edits
    }

    /// Get the maximum insertions allowed.
    #[must_use]
    pub fn get_insertions(&self) -> Option<NumEdits> {
        self.insertions
    }

    /// Maximum number of text characters insertions may consume at a single
    /// fuzzy position. A shared `{e<=k}` budget allows up to `k` insertions
    /// (plus any explicit `i` cap); an explicit `i` cap is used when no shared
    /// budget is present. `{s<=k}`/`{d<=k}` alone add no text.
    #[must_use]
    pub fn insertion_capacity(&self) -> NumEdits {
        match (self.edits, self.insertions) {
            (Some(e), Some(i)) => e.min(i),
            (Some(e), None) => e,
            (None, Some(i)) => i,
            (None, None) => 0,
        }
    }

    /// Get the maximum deletions allowed.
    #[must_use]
    pub fn get_deletions(&self) -> Option<NumEdits> {
        self.deletions
    }

    /// Get the maximum substitutions allowed.
    #[must_use]
    pub fn get_substitutions(&self) -> Option<NumEdits> {
        self.substitutions
    }

    /// Get the maximum swaps allowed.
    #[must_use]
    pub fn get_swaps(&self) -> Option<NumEdits> {
        self.swaps
    }
}

/// Per-operation minimum edit counts a fuzzy match must satisfy (mrab's
/// `{1<=s<=1}`, `{2<=i<=3}` range syntax). Each field is a lower bound on the
/// corresponding operation's count; `None` means no bound for that operation.
/// `total` bounds the sum of insertions/deletions/substitutions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinEdits {
    /// Minimum total edits.
    pub total: Option<NumEdits>,
    /// Minimum insertions.
    pub insertions: Option<NumEdits>,
    /// Minimum deletions.
    pub deletions: Option<NumEdits>,
    /// Minimum substitutions.
    pub substitutions: Option<NumEdits>,
}

impl MinEdits {
    /// Whether no *effective* minimum is specified. A bound of `0` (mrab's
    /// `{0<=e<=2}`) is always satisfied and must not count as a minimum, or the
    /// matcher's below-min trailing-insertion suppression would wrongly fire for
    /// patterns that are semantically equivalent to `{e<=2}`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total.is_none_or(|v| v == 0)
            && self.insertions.is_none_or(|v| v == 0)
            && self.deletions.is_none_or(|v| v == 0)
            && self.substitutions.is_none_or(|v| v == 0)
    }

    /// Whether counts `(i, d, s, t)` satisfy every minimum bound. The `total`
    /// bound counts all four operations (matching `EditCounts::total`), so a
    /// transposition contributes toward it; the per-operation bounds apply to
    /// their own counts (mrab has no transpositions, so `t` is 0 there).
    #[must_use]
    pub fn met_by(&self, i: u8, d: u8, s: u8, t: u8) -> bool {
        self.total
            .is_none_or(|min| i.saturating_add(d).saturating_add(s).saturating_add(t) >= min)
            && self.insertions.is_none_or(|v| i >= v)
            && self.deletions.is_none_or(|v| d >= v)
            && self.substitutions.is_none_or(|v| s >= v)
    }

    /// Whether `e` satisfies every minimum bound.
    #[must_use]
    pub fn met_by_counts(&self, e: &crate::engine::EditCounts) -> bool {
        self.met_by(e.insertions, e.deletions, e.substitutions, e.swaps)
    }
}

/// Penalty weights for different edit operations.
///
/// These weights are used to calculate a weighted edit distance where different
/// operations can have different costs. Lower penalties mean the operation is
/// considered "cheaper" during matching.
#[derive(Debug, Clone)]
pub struct FuzzyPenalties {
    /// Penalty for inserting a character (extra char in text).
    pub insertion: f32,
    /// Penalty for deleting a character (missing char from pattern).
    pub deletion: f32,
    /// Penalty for substituting a character.
    pub substitution: f32,
    /// Penalty for swapping adjacent characters.
    pub swap: f32,
}

impl Default for FuzzyPenalties {
    fn default() -> Self {
        let m = 1.3;
        Self {
            substitution: 1.1 * m,
            insertion: 0.4 * m,
            deletion: 0.7 * m,
            swap: 0.4 * m,
        }
    }
}

impl FuzzyPenalties {
    /// Set the insertion penalty.
    #[must_use]
    pub fn insertion(mut self, penalty: f32) -> Self {
        self.insertion = penalty;
        self
    }

    /// Set the deletion penalty.
    #[must_use]
    pub fn deletion(mut self, penalty: f32) -> Self {
        self.deletion = penalty;
        self
    }

    /// Set the substitution penalty.
    #[must_use]
    pub fn substitution(mut self, penalty: f32) -> Self {
        self.substitution = penalty;
        self
    }

    /// Set the swap penalty.
    #[must_use]
    pub fn swap(mut self, penalty: f32) -> Self {
        self.swap = penalty;
        self
    }
}
