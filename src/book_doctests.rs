//! Compile-checks for the book's Rust code blocks.
//!
//! Each book page is included as the docs of a `#[cfg(doctest)]` module, so
//! `cargo test --doc` compiles its ```rust blocks against the real crate
//! (with proper linking). `cfg(doctest)` means these modules exist ONLY when
//! rustdoc collects doctests — never in normal builds, `cargo doc`, or
//! downstream/published builds (so the `include_str!` of `book/` is never
//! evaluated when `book/` is absent).

#[cfg(doctest)]
#[doc = include_str!("../book/src/SUMMARY.md")]
mod book_summary {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/advanced.md")]
mod book_advanced {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/advanced_captures.md")]
mod book_advanced_captures {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/advanced_compat.md")]
mod book_advanced_compat {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/advanced_handlers.md")]
mod book_advanced_handlers {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/advanced_partial.md")]
mod book_advanced_partial {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/advanced_wordlists.md")]
mod book_advanced_wordlists {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/api_builder.md")]
mod book_api_builder {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/api_fuzzy_regex.md")]
mod book_api_fuzzy_regex {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/api_match.md")]
mod book_api_match {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/api_reference.md")]
mod book_api_reference {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/api_streaming.md")]
mod book_api_streaming {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/appendix_errors.md")]
mod book_appendix_errors {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/appendix_migration.md")]
mod book_appendix_migration {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/appendix_syntax.md")]
mod book_appendix_syntax {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/fuzzy_basics.md")]
mod book_fuzzy_basics {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/fuzzy_cost.md")]
mod book_fuzzy_cost {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/fuzzy_edit_distance.md")]
mod book_fuzzy_edit_distance {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/fuzzy_markers.md")]
mod book_fuzzy_markers {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/impl_bitap.md")]
mod book_impl_bitap {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/impl_bridge.md")]
mod book_impl_bridge {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/impl_dfa.md")]
mod book_impl_dfa {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/impl_nfa.md")]
mod book_impl_nfa {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/impl_overview.md")]
mod book_impl_overview {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/impl_streaming.md")]
mod book_impl_streaming {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/installation.md")]
mod book_installation {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/intro.md")]
mod book_intro {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_anchors.md")]
mod book_pattern_anchors {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_atomic.md")]
mod book_pattern_atomic {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_char_classes.md")]
mod book_pattern_char_classes {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_groups.md")]
mod book_pattern_groups {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_lookaround.md")]
mod book_pattern_lookaround {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_quantifiers.md")]
mod book_pattern_quantifiers {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/pattern_syntax.md")]
mod book_pattern_syntax {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/perf_algorithms.md")]
mod book_perf_algorithms {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/perf_simd.md")]
mod book_perf_simd {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/perf_tips.md")]
mod book_perf_tips {}

#[cfg(doctest)]
#[doc = include_str!("../book/src/quick_start.md")]
mod book_quick_start {}
