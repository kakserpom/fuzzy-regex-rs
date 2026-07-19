//! Feature-parity harness: reads `pattern\ttext` lines (percent-encoded) and
//! prints, per line, fuzzy-regex's result in a form comparable to mrab-regex:
//!
//!   `1\t<subs>,<ins>,<dels>\t<start>\t<end>`  on match (fuzzy_counts = mrab order)
//!   `0`                                        on no match
//!   `E`                                        pattern failed to compile
//!   `P`                                        panic while matching
//!
//! Paired with `examples/parity_probe.py`.

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
    std::panic::set_hook(Box::new(|_| {}));
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let pat = percent_decode(parts.next().unwrap_or(""));
        let text = percent_decode(parts.next().unwrap_or(""));

        let result = std::panic::catch_unwind(|| match FuzzyRegex::new(&pat) {
            Ok(re) => match re.find(&text) {
                Some(m) => {
                    let (s, i, d) = m.fuzzy_counts();
                    format!("1\t{s},{i},{d}\t{}\t{}", m.start(), m.end())
                }
                None => "0".to_string(),
            },
            Err(_) => "E".to_string(),
        })
        .unwrap_or_else(|_| "P".to_string());

        writeln!(out, "{result}").unwrap();
        out.flush().unwrap();
    }
}
