//! Parser module for fuzzy regex patterns.
//!
//! This module provides the lexer and parser for converting
//! fuzzy regex pattern strings into an AST.

pub mod ast;
mod core;
pub mod fullcase;
pub mod lexer;

pub use ast::*;
pub use core::{ParseResult, parse, parse_with_flags};
