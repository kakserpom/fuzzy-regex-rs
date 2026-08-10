//! Literal pattern extraction from HIR for fuzzy matching.

use crate::ir::{Hir, LiteralPattern};

/// Extract all literal patterns from a HIR tree.
///
/// These patterns will be compiled into Levenshtein automata
/// for efficient fuzzy matching.
#[must_use]
pub fn extract_literals(hir: &Hir) -> Vec<LiteralPattern> {
    let mut literals = Vec::new();
    extract_recursive(hir, &mut literals);
    literals
}

fn extract_recursive(hir: &Hir, out: &mut Vec<LiteralPattern>) {
    match hir {
        Hir::Literal {
            text,
            limits,
            min_edits,
            edit_chars,
            ..
        } => {
            out.push(LiteralPattern::with_edit_chars(
                text.clone(),
                limits.clone(),
                *min_edits,
                edit_chars.clone(),
            ));
        }

        Hir::Concat(parts) => {
            for part in parts {
                extract_recursive(part, out);
            }
        }

        Hir::Alt(alts) => {
            for alt in alts {
                extract_recursive(alt, out);
            }
        }

        Hir::Repeat { expr, .. }
        | Hir::Capture { expr, .. }
        | Hir::Lookahead { expr, .. }
        | Hir::Lookbehind { expr, .. } => {
            extract_recursive(expr, out);
        }

        Hir::FuzzyGroup { inner, .. } => {
            extract_recursive(inner, out);
        }

        // These don't contain literals
        // NamedList is handled at runtime when word lists are provided
        // ResetMatchStart doesn't produce literals
        // Recursive patterns - these can produce literals from the recursive call
        // For now, we'll handle them as potentially having literals
        Hir::Empty
        | Hir::Char(_)
        | Hir::Class(_)
        | Hir::FuzzyClass { .. }
        | Hir::Anchor(_)
        | Hir::Backreference { .. }
        | Hir::NamedList { .. }
        | Hir::ResetMatchStart
        | Hir::RecursivePattern { .. }
        | Hir::RecursiveGroup { .. }
        | Hir::RecursiveNamedGroup { .. }
        | Hir::Handler { .. } => {}

        // AtomicGroup - extract from inner expression
        Hir::AtomicGroup { expr } => extract_recursive(expr, out),
    }
}

/// Deduplicate literal patterns, keeping track of indices.
///
/// Returns the deduplicated list and a mapping from original index to deduped index.
#[must_use]
pub fn deduplicate_literals(literals: Vec<LiteralPattern>) -> (Vec<LiteralPattern>, Vec<usize>) {
    let mut deduped = Vec::new();
    let mut indices = Vec::new();
    let mut seen: crate::engine::FxHashMap<(String, String), usize> =
        crate::engine::FxHashMap::default();

    for lit in literals {
        let key = (lit.text.clone(), format!("{:?}", lit.limits));
        if let Some(&idx) = seen.get(&key) {
            indices.push(idx);
        } else {
            let idx = deduped.len();
            seen.insert(key, idx);
            indices.push(idx);
            deduped.push(lit);
        }
    }

    (deduped, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower;
    use crate::parser::parse;

    #[test]
    fn test_extract_simple() {
        let ast = parse("hello").unwrap();
        let hir = lower(&ast, 2);
        let literals = extract_literals(&hir);
        assert_eq!(literals.len(), 1);
        assert_eq!(literals[0].text, "hello");
    }

    #[test]
    fn test_extract_alternation() {
        let ast = parse("hello|world").unwrap();
        let hir = lower(&ast, 2);
        let literals = extract_literals(&hir);
        assert_eq!(literals.len(), 2);
    }

    #[test]
    fn test_extract_with_char_class() {
        let ast = parse(r"hello\d+world").unwrap();
        let hir = lower(&ast, 2);
        let literals = extract_literals(&hir);
        // Should extract "hello" and "world"
        assert_eq!(literals.len(), 2);
    }

    #[test]
    fn test_extract_nested() {
        let ast = parse("(hello)+").unwrap();
        let hir = lower(&ast, 2);
        let literals = extract_literals(&hir);
        assert_eq!(literals.len(), 1);
        assert_eq!(literals[0].text, "hello");
    }
}
