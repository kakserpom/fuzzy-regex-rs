//! Compiler module for transforming HIR to NFA.

pub mod literal_extractor;
pub mod nfa_builder;

pub use literal_extractor::{deduplicate_literals, extract_literals};
pub use nfa_builder::{NfaBuilder, build_nfa};
