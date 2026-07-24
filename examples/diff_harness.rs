//! Differential-fuzzing harness driver.
//!
//! Reads test cases from stdin, one per line, TSV-encoded:
//!     <pattern>\t<text>
//! where `pattern` and `text` are percent-encoded (only %XX escapes are decoded,
//! so the wire format never contains a raw TAB or NEWLINE).
//!
//! For each case it prints one line to stdout. The line is one of:
//!     E                       -> pattern failed to compile
//!     P                       -> panic while matching (via catch_unwind)
//!     <find>|<iter>           -> results of find() and find_iter().next()
//! where each of <find>/<iter> is either:
//!     N                       -> no match
//!     <start>,<end>,<su>,<in>,<de>   -> match byte span + fuzzy_counts (subs,ins,dels)
//!
//! Both find() and find_iter() are reported because they can legitimately (and,
//! per open bug, buggily) disagree; the Python side compares each against the
//! mrab oracle span so divergences can be attributed to the right entry point.
//!
//! Paired with `examples/diff_fuzz.py`, which uses Python's mrab `regex`
//! module as the oracle.

use fuzzy_regex::{FuzzyRegexBuilder, Match, MatchEndPolicy};
use std::io::{self, BufRead, Write};

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encode a match as `start,end,subs,ins,dels` (byte span), or `N` if absent.
fn encode(m: Option<Match<'_>>) -> String {
    match m {
        None => "N".to_string(),
        Some(m) => {
            let (su, in_, de) = m.fuzzy_counts();
            format!("{},{},{},{},{}", m.start(), m.end(), su, in_, de)
        }
    }
}

fn main() {
    // Silence panic backtraces; a panic is signalled to the orchestrator as `P`.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
    }));

    // MATCH_END_POLICY=minedit switches to MinEdit (tightest span) instead of the
    // default LongestWithinBudget, for measuring mrab-alignment of each policy.
    let min_edit = std::env::var("MATCH_END_POLICY").as_deref() == Ok("minedit");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let pat = percent_decode(parts.next().unwrap_or(""));
        let text = percent_decode(parts.next().unwrap_or(""));

        let result = std::panic::catch_unwind(|| {
            let mut builder = FuzzyRegexBuilder::new(&pat);
            if min_edit {
                builder = builder.match_end_policy(MatchEndPolicy::MinEdit);
            }
            match builder.build() {
                Ok(re) => {
                    let find = encode(re.find(&text));
                    let iter = encode(re.find_iter(&text).next());
                    format!("{find}|{iter}")
                }
                Err(_) => "E".to_string(),
            }
        })
        .unwrap_or_else(|_| "P".to_string());

        out.write_all(result.as_bytes()).unwrap();
        out.write_all(b"\n").unwrap();
        out.flush().unwrap();
    }
}
