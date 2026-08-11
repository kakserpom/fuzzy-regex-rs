//! Rust side of the fuzzy-regex vs mrab-regex comparison benchmark.
//!
//! Reads the shared case list `benches/compare_cases.tsv`, times each case, and
//! prints `RUST<TAB>name<TAB>ns_per_iter<TAB>result` lines. The Python side
//! (`benches/mrab_compare.py`) reads the same file; `benches/mrab_compare.sh`
//! runs both and prints a side-by-side table.
//!
//! Run with: cargo bench --bench mrab_compare

use fuzzy_regex::FuzzyRegex;
use std::hint::black_box;
use std::time::Instant;

/// The named text corpora. Must match `benches/mrab_compare.py` exactly.
fn corpora() -> Vec<(&'static str, String)> {
    let short = "The quick brown fox jumps over the lazy dog.".to_string();
    let medium = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris."
        .to_string();
    let long = medium.repeat(20);
    let dna = "ACGT".repeat(250);
    let repeats = "alpha beta gamma delta beta epsilon zeta beta".to_string();
    let code = "x(a(b)c)(d(e(f)g)h)y ".repeat(50);
    let unicode = "grüße die straße weiß im FUSSBALL".to_string();
    vec![
        ("short", short),
        ("medium", medium),
        ("long", long),
        ("dna", dna),
        ("repeats", repeats),
        ("code", code),
        ("unicode", unicode),
    ]
}

fn main() {
    let corpora = corpora();
    let find = |k: &str| -> &str {
        corpora
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("unknown corpus {k}"))
    };

    let tsv = std::fs::read_to_string("benches/compare_cases.tsv")
        .expect("run from the crate root; benches/compare_cases.tsv missing");

    for line in tsv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let mut it = line.splitn(5, '\t');
        let name = it.next().unwrap();
        let op = it.next().unwrap();
        let corpus = it.next().unwrap();
        let iters: u32 = it.next().unwrap().parse().unwrap();
        let pattern = it.next().unwrap();
        let text = find(corpus);

        let re = match FuzzyRegex::new(pattern) {
            Ok(re) => re,
            Err(e) => {
                println!("RUST\t{name}\tERR\tcompile: {e}");
                continue;
            }
        };

        // Warmup + a representative result to confirm both engines do real work.
        let result: String = match op {
            "find" => format!("{:?}", re.find(text).map(|m| (m.start(), m.end()))),
            "find_iter" => format!("n={}", re.find_iter(text).count()),
            "is_match" => format!("{}", re.is_match(text)),
            other => panic!("unknown op {other}"),
        };
        for _ in 0..3 {
            run_op(&re, op, text);
        }

        let start = Instant::now();
        for _ in 0..iters {
            run_op(&re, op, text);
        }
        let ns = start.elapsed().as_nanos() as f64 / f64::from(iters);
        println!("RUST\t{name}\t{ns:.1}\t{result}");
    }
}

#[inline]
fn run_op(re: &FuzzyRegex, op: &str, text: &str) {
    match op {
        "find" => {
            black_box(re.find(black_box(text)));
        }
        "find_iter" => {
            black_box(re.find_iter(black_box(text)).count());
        }
        "is_match" => {
            black_box(re.is_match(black_box(text)));
        }
        _ => unreachable!(),
    }
}
