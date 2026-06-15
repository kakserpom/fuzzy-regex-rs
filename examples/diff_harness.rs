//! Differential-fuzzing harness driver.
//!
//! Reads test cases from stdin, one per line, TSV-encoded:
//!     <pattern>\t<text>
//! where `pattern` and `text` are percent-encoded (only %XX escapes are decoded,
//! so the wire format never contains a raw TAB or NEWLINE).
//!
//! For each case it prints one line to stdout:
//!     1  -> fuzzy-regex reports a match (find() is Some)
//!     0  -> no match
//!     E  -> pattern failed to compile
//!     P  -> panic while matching (printed via catch_unwind)
//!
//! Paired with `examples/diff_fuzz.py`, which uses Python's mrab `regex`
//! module as the oracle.

use fuzzy_regex::FuzzyRegex;
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

fn main() {
    // Silence panic backtraces; a panic is signalled to the orchestrator as `P`.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
    }));

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

        let result = std::panic::catch_unwind(|| match FuzzyRegex::new(&pat) {
            Ok(re) => {
                if re.find(&text).is_some() {
                    b'1'
                } else {
                    b'0'
                }
            }
            Err(_) => b'E',
        })
        .unwrap_or(b'P');

        out.write_all(&[result, b'\n']).unwrap();
        out.flush().unwrap();
    }
}
