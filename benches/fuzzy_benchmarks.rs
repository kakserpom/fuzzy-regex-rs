use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fuzzy_regex::{FuzzyRegex, FuzzyRegexBuilder};
use regex::Regex;
use std::hint::black_box;

// Sample texts of varying sizes
const SHORT_TEXT: &str = "The quick brown fox jumps over the lazy dog.";
const MEDIUM_TEXT: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
    Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
    Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
    nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
    reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";

fn generate_long_text() -> String {
    MEDIUM_TEXT.repeat(100)
}

fn generate_very_long_text() -> String {
    MEDIUM_TEXT.repeat(1000)
}

// Benchmark: Simple exact match (baseline)
fn bench_exact_match(c: &mut Criterion) {
    let re = FuzzyRegex::new("quick").unwrap();

    c.bench_function("exact_match_short", |b| {
        b.iter(|| re.find(black_box(SHORT_TEXT)));
    });
}

// Benchmark: Fuzzy match with 1 edit
fn bench_fuzzy_1_edit(c: &mut Criterion) {
    let re = FuzzyRegex::new("(?:quikc){e<=1}").unwrap();

    c.bench_function("fuzzy_1_edit_short", |b| {
        b.iter(|| re.find(black_box(SHORT_TEXT)));
    });
}

// Benchmark: Fuzzy match with 2 edits
fn bench_fuzzy_2_edits(c: &mut Criterion) {
    let re = FuzzyRegex::new("(?:qwick){e<=2}").unwrap();

    c.bench_function("fuzzy_2_edits_short", |b| {
        b.iter(|| re.find(black_box(SHORT_TEXT)));
    });
}

// Benchmark: Fuzzy match with substitution constraint
fn bench_fuzzy_substitution(c: &mut Criterion) {
    let re = FuzzyRegex::new("(?:quack){s<=2}").unwrap();

    c.bench_function("fuzzy_substitution_short", |b| {
        b.iter(|| re.find(black_box(SHORT_TEXT)));
    });
}

// Benchmark: Fuzzy match with cost constraint
fn bench_fuzzy_cost_constraint(c: &mut Criterion) {
    let re = FuzzyRegex::new("(?:quikc){1i+1d<3}").unwrap();

    c.bench_function("fuzzy_cost_constraint_short", |b| {
        b.iter(|| re.find(black_box(SHORT_TEXT)));
    });
}

// Benchmark: Text size scaling
fn bench_text_size_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_size_scaling");

    let re = FuzzyRegex::new("(?:lorem){e<=2}").unwrap();

    let long_text = generate_long_text();
    let very_long_text = generate_very_long_text();

    group.throughput(Throughput::Bytes(MEDIUM_TEXT.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("fuzzy_2_edits", "medium"),
        MEDIUM_TEXT,
        |b, text| b.iter(|| re.find(black_box(text))),
    );

    group.throughput(Throughput::Bytes(long_text.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("fuzzy_2_edits", "long"),
        &long_text,
        |b, text| b.iter(|| re.find(black_box(text))),
    );

    group.throughput(Throughput::Bytes(very_long_text.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("fuzzy_2_edits", "very_long"),
        &very_long_text,
        |b, text| b.iter(|| re.find(black_box(text))),
    );

    group.finish();
}

// Benchmark: Pattern length scaling
fn bench_pattern_length_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_length_scaling");

    let long_text = generate_long_text();

    // Short pattern (5 chars)
    let re_short = FuzzyRegex::new("(?:lorem){e<=1}").unwrap();
    group.bench_function("pattern_5_chars", |b| {
        b.iter(|| re_short.find(black_box(&long_text)));
    });

    // Medium pattern (10 chars)
    let re_medium = FuzzyRegex::new("(?:consectetur){e<=2}").unwrap();
    group.bench_function("pattern_11_chars", |b| {
        b.iter(|| re_medium.find(black_box(&long_text)));
    });

    // Longer pattern (15+ chars)
    let re_long = FuzzyRegex::new("(?:exercitation){e<=2}").unwrap();
    group.bench_function("pattern_13_chars", |b| {
        b.iter(|| re_long.find(black_box(&long_text)));
    });

    group.finish();
}

// Benchmark: Edit distance scaling
fn bench_edit_distance_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit_distance_scaling");

    let long_text = generate_long_text();

    // 0 edits (exact)
    let re_0 = FuzzyRegex::new("(?:lorem){e<=0}").unwrap();
    group.bench_function("0_edits", |b| {
        b.iter(|| re_0.find(black_box(&long_text)));
    });

    // 1 edit
    let re_1 = FuzzyRegex::new("(?:lorem){e<=1}").unwrap();
    group.bench_function("1_edit", |b| {
        b.iter(|| re_1.find(black_box(&long_text)));
    });

    // 2 edits
    let re_2 = FuzzyRegex::new("(?:lorem){e<=2}").unwrap();
    group.bench_function("2_edits", |b| {
        b.iter(|| re_2.find(black_box(&long_text)));
    });

    // 3 edits
    let re_3 = FuzzyRegex::new("(?:lorem){e<=3}").unwrap();
    group.bench_function("3_edits", |b| {
        b.iter(|| re_3.find(black_box(&long_text)));
    });

    group.finish();
}

// Benchmark: find_iter (multiple matches)
fn bench_find_iter(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_iter");

    let long_text = generate_long_text();

    let re = FuzzyRegex::new("(?:dolor){e<=1}").unwrap();

    group.bench_function("find_iter_long_text", |b| {
        b.iter(|| {
            let count: usize = re.find_iter(black_box(&long_text)).count();
            black_box(count)
        });
    });

    group.finish();
}

// Benchmark: is_match (boolean check)
fn bench_is_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("is_match");

    let long_text = generate_long_text();

    let re = FuzzyRegex::new("(?:lorem){e<=2}").unwrap();

    group.bench_function("is_match_found", |b| {
        b.iter(|| re.is_match(black_box(&long_text)));
    });

    let re_no_match = FuzzyRegex::new("(?:xyzzy){e<=1}").unwrap();
    group.bench_function("is_match_not_found", |b| {
        b.iter(|| re_no_match.is_match(black_box(&long_text)));
    });

    group.finish();
}

// Benchmark: Regex compilation
fn bench_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("compilation");

    group.bench_function("simple_pattern", |b| {
        b.iter(|| FuzzyRegex::new(black_box("(?:hello){e<=2}")));
    });

    group.bench_function("complex_pattern", |b| {
        b.iter(|| FuzzyRegex::new(black_box("(?:hello){i<=1,d<=1,s<=2,1i+1d<3}")));
    });

    group.finish();
}

// Benchmark: Real-world scenario - typo correction
fn bench_typo_correction(c: &mut Criterion) {
    let mut group = c.benchmark_group("typo_correction");

    let document = "The recieve function should recieve data from the server. \
        Make sure to recieve all packets before processing. \
        If you don't recieve a response within 5 seconds, retry.";

    // Find misspellings of "receive"
    let re = FuzzyRegex::new("(?:receive){e<=2}").unwrap();

    group.bench_function("find_misspellings", |b| {
        b.iter(|| {
            let matches: Vec<_> = re.find_iter(black_box(document)).collect();
            black_box(matches)
        });
    });

    group.finish();
}

// Benchmark: DNA sequence matching (bioinformatics use case)
fn bench_dna_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("dna_matching");

    // Generate a pseudo-random DNA sequence
    let dna: String = (0..10000)
        .map(|i| match i % 4 {
            0 => 'A',
            1 => 'C',
            2 => 'G',
            _ => 'T',
        })
        .collect();

    // Search for a motif with fuzzy matching
    let re = FuzzyRegex::new("(?:ACGTACGT){e<=2}").unwrap();

    group.throughput(Throughput::Bytes(dna.len() as u64));
    group.bench_function("find_motif", |b| {
        b.iter(|| re.find(black_box(&dna)));
    });

    group.finish();
}

// Benchmark: Compare against regex crate
fn bench_vs_regex(c: &mut Criterion) {
    let mut group = c.benchmark_group("vs_regex");

    let long_text = generate_long_text();

    // Literal search
    {
        let fr = FuzzyRegex::new("lorem").unwrap();
        let rr = Regex::new("lorem").unwrap();

        group.bench_function("literal/fuzzy-regex", |b| {
            b.iter(|| fr.find(black_box(&long_text)));
        });
        group.bench_function("literal/regex", |b| {
            b.iter(|| rr.find(black_box(&long_text)));
        });
    }

    // Character class [a-z]+
    {
        let fr = FuzzyRegexBuilder::new("[a-z]+").build().unwrap();
        let rr = Regex::new("[a-z]+").unwrap();

        group.bench_function("char_class/fuzzy-regex", |b| {
            b.iter(|| fr.find(black_box(&long_text)));
        });
        group.bench_function("char_class/regex", |b| {
            b.iter(|| rr.find(black_box(&long_text)));
        });
    }

    // Word class \w+
    {
        let fr = FuzzyRegexBuilder::new(r"\w+").build().unwrap();
        let rr = Regex::new(r"\w+").unwrap();

        group.bench_function("word_class/fuzzy-regex", |b| {
            b.iter(|| fr.find(black_box(&long_text)));
        });
        group.bench_function("word_class/regex", |b| {
            b.iter(|| rr.find(black_box(&long_text)));
        });
    }

    // Anchored ^Lorem
    {
        let fr = FuzzyRegexBuilder::new("^Lorem").build().unwrap();
        let rr = Regex::new("^Lorem").unwrap();

        group.bench_function("anchored/fuzzy-regex", |b| {
            b.iter(|| fr.find(black_box(&long_text)));
        });
        group.bench_function("anchored/regex", |b| {
            b.iter(|| rr.find(black_box(&long_text)));
        });
    }

    // Alternation
    {
        let fr = FuzzyRegexBuilder::new("lorem|ipsum|dolor").build().unwrap();
        let rr = Regex::new("lorem|ipsum|dolor").unwrap();

        group.bench_function("alternation/fuzzy-regex", |b| {
            b.iter(|| fr.find(black_box(&long_text)));
        });
        group.bench_function("alternation/regex", |b| {
            b.iter(|| rr.find(black_box(&long_text)));
        });
    }

    // Case insensitive
    {
        let fr = FuzzyRegexBuilder::new("lorem")
            .case_insensitive(true)
            .build()
            .unwrap();
        let rr = Regex::new("(?i)lorem").unwrap();

        group.bench_function("case_insensitive/fuzzy-regex", |b| {
            b.iter(|| fr.find(black_box(&long_text)));
        });
        group.bench_function("case_insensitive/regex", |b| {
            b.iter(|| rr.find(black_box(&long_text)));
        });
    }

    // find_iter
    {
        let fr = FuzzyRegexBuilder::new("[a-z]+").build().unwrap();
        let rr = Regex::new("[a-z]+").unwrap();

        group.bench_function("find_iter/fuzzy-regex", |b| {
            b.iter(|| fr.find_iter(black_box(&long_text)).count());
        });
        group.bench_function("find_iter/regex", |b| {
            b.iter(|| rr.find_iter(black_box(&long_text)).count());
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_exact_match,
    bench_fuzzy_1_edit,
    bench_fuzzy_2_edits,
    bench_fuzzy_substitution,
    bench_fuzzy_cost_constraint,
    bench_text_size_scaling,
    bench_pattern_length_scaling,
    bench_edit_distance_scaling,
    bench_find_iter,
    bench_is_match,
    bench_compilation,
    bench_typo_correction,
    bench_dna_matching,
    bench_vs_regex,
);

criterion_main!(benches);
