//! Head-to-head: fuzzy-regex's fuzzy-aho-corasick compat vs the real
//! `fuzzy-aho-corasick` crate (heavily optimized), on identical workloads.
//!
//! Run: cargo run --release --example compare_fac_real
//!
//! Workloads mirror the real crate's own `benches/benchmark.rs` so the
//! comparison is apples-to-apples (same patterns, text, edit budget, threshold,
//! and the same `search_non_overlapping` entry point).

use std::hint::black_box;
use std::time::Instant;

// Real, heavily-optimized crate.
use fuzzy_aho_corasick::{FuzzyAhoCorasickBuilder as RealBuilder, FuzzyLimits as RealLimits};
// fuzzy-regex's reimplementation of the same API on top of its regex engine.
use fuzzy_regex::compat::FuzzyLimits as FrLimits;
use fuzzy_regex::compat::fac::FuzzyAhoCorasickBuilder as FrBuilder;

fn time<F: FnMut() -> usize>(iters: u32, mut f: F) -> (f64, usize) {
    for _ in 0..(iters / 10).max(5) {
        black_box(f());
    }
    let start = Instant::now();
    let mut last = 0;
    for _ in 0..iters {
        last = black_box(f());
    }
    let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(iters);
    (us, last)
}

fn row(name: &str, iters: u32, real: impl Fn() -> usize, fr: impl Fn() -> usize) {
    let (r_us, r_n) = time(iters, &real);
    let (f_us, f_n) = time(iters, &fr);
    let ratio = f_us / r_us;
    let verdict = if ratio <= 1.0 {
        format!("fuzzy-regex {:.2}x FASTER", 1.0 / ratio)
    } else {
        format!("fuzzy-regex {ratio:.2}x slower")
    };
    let flag = if r_n == f_n {
        ""
    } else {
        " [MATCH COUNT DIFFERS!]"
    };
    println!(
        "{name:22} real={r_us:8.3}us  fuzzy-regex={f_us:8.3}us  ({verdict})  matches r={r_n} f={f_n}{flag}"
    );
}

fn main() {
    println!("fuzzy-regex (compat) vs real fuzzy-aho-corasick v0.3.7 — search_non_overlapping\n");

    // 1. basic
    {
        let text = "this is a saddamhu example with multiple saddam matches and ddamhu too";
        let real = RealBuilder::new()
            .fuzzy(RealLimits::new().edits(2))
            .build(["saddam", "ddamhu"]);
        let fr = FrBuilder::new()
            .fuzzy(FrLimits::new().edits(2))
            .build(["saddam", "ddamhu"]);
        row(
            "basic (2 pat, e<=2)",
            20_000,
            || real.search_non_overlapping(black_box(text), 0.8).len(),
            || fr.search_non_overlapping(black_box(text), 0.8).len(),
        );
    }

    // 2. long text
    {
        let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut, commodo accumsan mi. Vestibulum porta, orci nec ullamcorper posuere, eros tortor pharetra est, at porttitor mi leo a velit. Aenean sollicitudin mauris elit, ultricies congue dui vulputate in. In hac habitasse platea dictumst. Nam iaculis sagittis justo a condimentum. Curabitur sed rhoncus dolor. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus egestas congue lorem, in convallis magna viverra quis.";
        let pats = ["tincidunt", "porta", "lorem", "ipsum"];
        let real = RealBuilder::new()
            .fuzzy(RealLimits::new().edits(1))
            .case_insensitive(true)
            .build(pats);
        let fr = FrBuilder::new()
            .fuzzy(FrLimits::new().edits(1))
            .case_insensitive(true)
            .build(pats);
        row(
            "long_text (4 pat)",
            5_000,
            || real.search_non_overlapping(black_box(text), 0.8).len(),
            || fr.search_non_overlapping(black_box(text), 0.8).len(),
        );

        // 2b. very long (10x)
        let vlong = text.repeat(10);
        row(
            "very_long_text (10x)",
            1_000,
            || real.search_non_overlapping(black_box(&vlong), 0.8).len(),
            || fr.search_non_overlapping(black_box(&vlong), 0.8).len(),
        );
    }

    // 3. many patterns
    {
        let pats = [
            "JOINT",
            "STOCK",
            "COMPANY",
            "LIMITED",
            "LIABILITY",
            "PUBLIC",
            "PRIVATE",
            "CORPORATION",
            "INTERNATIONAL",
            "ENTERPRISE",
            "TRADING",
            "HOLDINGS",
            "INVESTMENT",
            "CAPITAL",
            "PARTNERS",
            "ASSOCIATES",
            "SOLUTIONS",
            "INDUSTRIES",
            "TECHNOLOGIES",
            "SERVICES",
        ];
        let text = "PUBLIC JOINT STOCK COMPANY GAZPROM INTERNATIONAL HOLDINGS LIMITED LIABILITY";
        let real = RealBuilder::new()
            .fuzzy(RealLimits::new().edits(1))
            .case_insensitive(true)
            .build(pats);
        let fr = FrBuilder::new()
            .fuzzy(FrLimits::new().edits(1))
            .case_insensitive(true)
            .build(pats);
        row(
            "many_patterns (20)",
            5_000,
            || real.search_non_overlapping(black_box(text), 0.7).len(),
            || fr.search_non_overlapping(black_box(text), 0.7).len(),
        );
    }

    // 3b. unicode text (exercises the non-ASCII distance gate)
    {
        let text = "Съешь же ещё этих мягких французских булок да выпей чаю. Ελληνικά κείμενο εδώ. 日本語のテキストもここにある。café résumé naïve";
        let pats = ["французских", "κείμενο", "résumé"];
        let real = RealBuilder::new()
            .fuzzy(RealLimits::new().edits(2))
            .case_insensitive(true)
            .build(pats);
        let fr = FrBuilder::new()
            .fuzzy(FrLimits::new().edits(2))
            .case_insensitive(true)
            .build(pats);
        row(
            "unicode (3 pat, e<=2)",
            5_000,
            || real.search_non_overlapping(black_box(text), 0.7).len(),
            || fr.search_non_overlapping(black_box(text), 0.7).len(),
        );
    }

    // 4. fuzzy levels
    {
        let text = "this is a saddamhu example with multiple saddam matches";
        for edits in [1u8, 2, 3] {
            let real = RealBuilder::new()
                .fuzzy(RealLimits::new().edits(edits))
                .build(["saddam", "hussein"]);
            let fr = FrBuilder::new()
                .fuzzy(FrLimits::new().edits(edits))
                .build(["saddam", "hussein"]);
            row(
                &format!("fuzzy_levels e<={edits}"),
                10_000,
                || real.search_non_overlapping(black_box(text), 0.6).len(),
                || fr.search_non_overlapping(black_box(text), 0.6).len(),
            );
        }
    }
}
