//! Capture group handling during matching.

use std::sync::{Arc, OnceLock};

use super::hash::FxHashMap;

/// Type alias for capture group slots.
type CaptureSlots = Vec<Option<(usize, usize)>>;

/// Type alias for string storage (using String for better performance).
type Str = String;

/// Static empty `FxHashMap` to avoid allocation for patterns without named groups.
static EMPTY_NAMES: OnceLock<Arc<FxHashMap<Str, usize>>> = OnceLock::new();

fn empty_names() -> Arc<FxHashMap<Str, usize>> {
    EMPTY_NAMES
        .get_or_init(|| Arc::new(FxHashMap::default()))
        .clone()
}

/// Capture group state during matching.
#[derive(Debug, Clone)]
pub struct CaptureState {
    /// Capture slots: (start, end) for each group (0 = full match).
    slots: CaptureSlots,
    /// Named group mapping (shared across clones - never mutated after setup).
    names: Arc<FxHashMap<String, usize>>,
    /// Handler overrides: (`start_pos`, `end_pos`, `override_text`)
    /// Kept as Vec since most patterns don't use handlers.
    handler_overrides: Vec<(usize, usize, String)>,
}

impl CaptureState {
    /// Create a new capture state for n groups.
    #[must_use]
    pub fn new(group_count: usize) -> Self {
        let mut slots = CaptureSlots::new();
        slots.resize(group_count + 1, None); // +1 for group 0 (full match)
        CaptureState {
            slots,
            names: empty_names(),
            handler_overrides: Vec::new(),
        }
    }

    /// Set handler overrides.
    pub fn set_handler_overrides(&mut self, overrides: Vec<(usize, usize, String)>) {
        self.handler_overrides = overrides;
    }

    /// Get handler overrides.
    #[must_use]
    pub fn handler_overrides(&self) -> &[(usize, usize, String)] {
        &self.handler_overrides
    }

    /// Register a named group.
    pub fn register_name(&mut self, name: String, index: usize) {
        Arc::make_mut(&mut self.names).insert(name, index);
    }

    /// Start a capture at a position.
    pub fn start_capture(&mut self, group: usize, pos: usize) {
        if group < self.slots.len() {
            self.slots[group] = Some((pos, pos));
        }
    }

    /// End a capture at a position.
    pub fn end_capture(&mut self, group: usize, pos: usize) {
        if group < self.slots.len()
            && let Some((start, _)) = self.slots[group]
        {
            self.slots[group] = Some((start, pos));
        }
    }

    /// Get a capture by index.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<(usize, usize)> {
        self.slots.get(index).copied().flatten()
    }

    /// Get a capture by name.
    #[must_use]
    pub fn get_named(&self, name: &str) -> Option<(usize, usize)> {
        self.names.get(name).and_then(|&idx| self.get(idx))
    }

    /// Get the captured text for a group.
    #[must_use]
    pub fn get_text(&self, index: usize, text: &str) -> Option<String> {
        self.get(index).map(|(start, end)| {
            if self.handler_overrides.is_empty() {
                text[start..end].into()
            } else {
                self.apply_overrides(text, start, end)
            }
        })
    }

    fn apply_overrides(&self, text: &str, start: usize, end: usize) -> String {
        let mut result = String::new();
        let mut pos = start;

        for (ov_start, ov_end, ov_text) in &self.handler_overrides {
            let ov_start = *ov_start;
            let ov_end = *ov_end;

            if ov_end <= start || ov_start >= end {
                continue;
            }

            if ov_start > pos {
                result.push_str(&text[pos..ov_start.min(end)]);
            }

            let overlap_start = ov_start.max(start);
            let overlap_end = ov_end.min(end);
            if overlap_start < overlap_end {
                result.push_str(ov_text);
            }

            pos = ov_end;
        }

        if pos < end {
            result.push_str(&text[pos..end]);
        }

        result
    }

    /// Set the full match (group 0).
    pub fn set_full_match(&mut self, start: usize, end: usize) {
        self.slots[0] = Some((start, end));
    }

    /// Get all slots.
    #[must_use]
    pub fn slots(&self) -> &[Option<(usize, usize)>] {
        &self.slots
    }

    /// Get the name mapping.
    #[must_use]
    pub fn names(&self) -> &FxHashMap<String, usize> {
        &self.names
    }

    /// Clear all captures (but keep structure).
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
    }

    /// Clone with a new position offset.
    #[must_use]
    pub fn with_offset(&self, offset: usize) -> Self {
        CaptureState {
            slots: self
                .slots
                .iter()
                .map(|s| s.map(|(start, end)| (start + offset, end + offset)))
                .collect(),
            names: Arc::clone(&self.names),
            handler_overrides: self.handler_overrides.clone(),
        }
    }
}

/// Builder for constructing capture state with names.
pub struct CaptureStateBuilder {
    group_count: usize,
    names: FxHashMap<String, usize>,
}

impl CaptureStateBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new(group_count: usize) -> Self {
        CaptureStateBuilder {
            group_count,
            names: FxHashMap::default(),
        }
    }

    /// Add a named group.
    #[must_use]
    pub fn with_name(mut self, name: String, index: usize) -> Self {
        self.names.insert(name, index);
        self
    }

    /// Build the capture state.
    #[must_use]
    pub fn build(self) -> CaptureState {
        let mut slots = CaptureSlots::new();
        slots.resize(self.group_count + 1, None);
        CaptureState {
            slots,
            names: Arc::new(self.names),
            handler_overrides: Vec::new(),
        }
    }
}
