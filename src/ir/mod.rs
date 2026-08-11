//! Intermediate Representation module.
//!
//! This module contains:
//! - HIR (High-level IR): simplified AST ready for compilation
//! - NFA: Non-deterministic Finite Automaton for matching

// Module-level allows for IR-specific patterns

pub mod hir;
pub mod nfa;

pub use hir::{CostInfo, Hir, HirClass, HirLowering, lower, lower_with_unicode};
pub use nfa::{
    CostConstraint, EditCharRestriction, LiteralPattern, Nfa, NfaFragment, PatternIndex,
    PrefixDotStarSuffix, State, StateId,
};
