//! Table-driven test corpus (TOML), inspired by resharp's `tests/*.toml`.
//!
//! Each file under `tests/corpus/*.toml` holds a `description` and a list of
//! `[[test]]` cases. Cases are declarative — pattern, input, and the expected
//! outcome (match span, `fuzzy_counts`, or compile error) — so feature coverage
//! and regressions are auditable and cheap to extend without writing Rust.
//!
//! Case fields (all optional except `pattern`):
//!   name             — label shown on failure
//!   pattern          — the regex (required)
//!   input            — haystack (default "")
//!   threshold        — similarity threshold (default 0.0)
//!   case_insensitive — build case-insensitively (default false)
//!   expect_error     — the pattern must fail to compile
//!   is_match         — whether `find` must return a match
//!   span             — expected [start, end] byte span of `find`
//!   fuzzy_counts     — expected (substitutions, insertions, deletions) — mrab order
//!   ignore           — skip this case

use fuzzy_regex::{FuzzyRegexBuilder, MatchEndPolicy};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Corpus {
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    #[serde(default)]
    test: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    #[serde(default)]
    name: String,
    pattern: String,
    #[serde(default)]
    input: String,
    #[serde(default)]
    threshold: Option<f32>,
    #[serde(default)]
    case_insensitive: bool,
    /// Select `MatchEndPolicy::MinEdit` (tightest alignment) instead of the
    /// default longest-within-budget end selection.
    #[serde(default)]
    min_edit: bool,
    #[serde(default)]
    expect_error: bool,
    #[serde(default)]
    is_match: Option<bool>,
    #[serde(default)]
    span: Option<[usize; 2]>,
    #[serde(default)]
    fuzzy_counts: Option<[u32; 3]>,
    #[serde(default)]
    ignore: bool,
}

/// Run a single case, returning `Err(reason)` on failure.
fn run_case(c: &Case) -> Result<(), String> {
    let policy = if c.min_edit {
        MatchEndPolicy::MinEdit
    } else {
        MatchEndPolicy::LongestWithinBudget
    };
    let built = FuzzyRegexBuilder::new(&c.pattern)
        .case_insensitive(c.case_insensitive)
        .similarity(c.threshold.unwrap_or(0.0))
        .match_end_policy(policy)
        .build();

    if c.expect_error {
        return match built {
            Ok(_) => Err("expected compile error, but pattern compiled".into()),
            Err(_) => Ok(()),
        };
    }

    let re = built.map_err(|e| format!("unexpected compile error: {e:?}"))?;
    let m = re.find(&c.input);

    if let Some([s, e]) = c.span {
        match &m {
            Some(m) if (m.start(), m.end()) == (s, e) => {}
            Some(m) => {
                return Err(format!(
                    "span mismatch: expected [{s}, {e}], got [{}, {}]",
                    m.start(),
                    m.end()
                ));
            }
            None => return Err(format!("span mismatch: expected [{s}, {e}], got no match")),
        }
    }

    if let Some(want) = c.is_match
        && m.is_some() != want
    {
        return Err(format!("is_match: expected {want}, got {}", m.is_some()));
    }

    if let Some([sub, ins, del]) = c.fuzzy_counts {
        match &m {
            Some(m) if m.fuzzy_counts() == (sub, ins, del) => {}
            Some(m) => {
                return Err(format!(
                    "fuzzy_counts mismatch: expected ({sub}, {ins}, {del}), got {:?}",
                    m.fuzzy_counts()
                ));
            }
            None => {
                return Err(format!(
                    "fuzzy_counts: expected ({sub}, {ins}, {del}), got no match"
                ));
            }
        }
    }

    Ok(())
}

#[test]
fn toml_corpus() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no .toml corpus files found in {}",
        dir.display()
    );

    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for path in &files {
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(path).unwrap();
        let corpus: Corpus =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{file_name}: TOML parse error: {e}"));
        for (i, c) in corpus.test.iter().enumerate() {
            if c.ignore {
                continue;
            }
            total += 1;
            if let Err(reason) = run_case(c) {
                let label = if c.name.is_empty() {
                    format!("#{i}")
                } else {
                    c.name.clone()
                };
                failures.push(format!(
                    "  [{file_name}] {label}: {reason}\n    pattern={:?} input={:?}",
                    c.pattern, c.input
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {total} corpus cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!(
        "toml_corpus: {total} cases passed across {} files",
        files.len()
    );
}
