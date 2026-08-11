//! Comprehensive benchmark comparing fuzzy-regex with mrab-regex
//!
//! Run with: cargo run --release --example `compare_full`
//!
//! Then run: python3 `benches/compare_full.py`
//! to get mrab-regex results for comparison.

use fuzzy_regex::FuzzyRegexBuilder;
use std::time::Instant;

fn bench<F: FnMut() -> bool>(name: &str, iterations: u32, mut f: F) -> f64 {
    // Warmup
    for _ in 0..100 {
        std::hint::black_box(f());
    }

    let start = Instant::now();
    let mut found = 0u32;
    for _ in 0..iterations {
        if f() {
            found += 1;
        }
    }
    let elapsed = start.elapsed();

    let per_iter_us = elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations);
    println!("{name:55} {per_iter_us:>8.2} us  (found: {found}/{iterations})");
    per_iter_us
}

fn bench_text_length_scaling() {
    println!("--- 1. Text Length Scaling (pattern at start) ---");
    let base_text = "The quick brown fox jumps over the lazy dog. ";
    for len in [50, 100, 500, 1000, 5000, 10_000] {
        let text: String = base_text.chars().cycle().take(len).collect();
        let regex = FuzzyRegexBuilder::new("(?:quick){e<=1}")
            .build()
            .unwrap();
        bench(&format!("'quick' e<=1 in {len} chars"), 10_000, || regex.find(&text).is_some());
    }
}

fn bench_match_position_impact() {
    println!("\n--- 2. Match Position Impact (1000 char text) ---");
    let base_text: String = "X".repeat(1000);
    for pos in [0, 10, 50, 100, 500, 900] {
        let mut text = base_text.clone();
        text.replace_range(pos..pos + 5, "quick");
        let regex = FuzzyRegexBuilder::new("(?:quick){e<=1}")
            .build()
            .unwrap();
        bench(&format!("'quick' at position {pos}"), 10_000, || regex.find(&text).is_some());
    }
}

fn bench_edit_distance_scaling() {
    println!("\n--- 3. Edit Distance Scaling ---");
    let text = "The quikc brown fox jumps over the lazy dog.";
    for edit_dist in 0..=4 {
        let pattern = format!("(?:quick){{e<={edit_dist}}}");
        let regex = FuzzyRegexBuilder::new(&pattern)
            .build()
            .unwrap();
        bench(&format!("'quick' e<={edit_dist}"), 10_000, || regex.find(text).is_some());
    }
}

fn bench_pattern_length_scaling() {
    println!("\n--- 4. Pattern Length Scaling (e<=2) ---");
    let long_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(20);
    let patterns = [
        ("Lorem", 5),
        ("consectetur", 11),
        ("adipiscing elit", 15),
        ("Lorem ipsum dolor", 17),
        ("consectetur adipiscing", 22),
    ];
    for (pat, len) in patterns {
        let pattern = format!("(?:{pat}){{e<=2}}");
        let regex = FuzzyRegexBuilder::new(&pattern)
            .build()
            .unwrap();
        bench(&format!("'{}' ({} chars) e<=2", &pat[..pat.len().min(15)], len), 10_000, || regex.find(&long_text).is_some());
    }
}

fn bench_dna_bioinformatics() {
    println!("\n--- 5. DNA/Bioinformatics ---");
    let dna: String = (0..10_000).map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' }).collect();

    // Different motif lengths
    for motif_len in [4, 8, 12, 16, 20] {
        let motif: String = (0..motif_len).map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' }).collect();
        let pattern = format!("(?:{motif}){{e<=2}}");
        let regex = FuzzyRegexBuilder::new(&pattern)
            .build()
            .unwrap();
        bench(&format!("DNA motif {motif_len} bp, e<=2, 10kb"), 1_000, || regex.find(&dna).is_some());
    }
}

fn bench_dna_size_scaling() {
    println!("\n--- 5b. DNA Size Scaling ---");
    let motif = "ACGTACGT";
    for size in [100, 1000, 10_000, 100_000] {
        let dna: String = (0..size).map(|i| match i % 4 { 0 => 'A', 1 => 'C', 2 => 'G', _ => 'T' }).collect();
        let pattern = format!("(?:{motif}){{e<=2}}");
        let regex = FuzzyRegexBuilder::new(&pattern)
            .build()
            .unwrap();
        let iters = if size > 10_000 { 100 } else { 1_000 };
        bench(&format!("ACGTACGT e<=2 in {size} bp"), iters, || regex.find(&dna).is_some());
    }
}

fn bench_no_match() {
    println!("\n--- 6. No Match (Full Scan) ---");
    for size in [100, 500, 1000, 5000] {
        let text: String = "X".repeat(size);
        let regex = FuzzyRegexBuilder::new("(?:quick){e<=1}")
            .build()
            .unwrap();
        let iters = if size > 1000 { 1_000 } else { 10_000 };
        bench(&format!("No match in {size} chars"), iters, || regex.find(&text).is_some());
    }
}

fn bench_alternation_patterns() {
    println!("\n--- 7. Alternation Patterns ---");
    let text = "The quick brown fox jumps over the lazy dog.";

    let alt_patterns = [
        ("(?:quick|slow){e<=1}", "2 alts, short"),
        ("(?:quick|brown|lazy){e<=1}", "3 alts, short"),
        ("(?:the|quick|brown|fox|jumps){e<=1}", "5 alts, short"),
        ("(?:quick|brown|fox|jumps|over|lazy|dog|the|a|an){e<=1}", "10 alts"),
    ];
    for (pattern, description) in alt_patterns {
        let regex = FuzzyRegexBuilder::new(pattern)
            .build()
            .unwrap();
        bench(description, 10_000, || regex.find(text).is_some());
    }
}

fn bench_case_insensitive() {
    println!("\n--- 8. Case Insensitive ---");
    let text = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG.";
    let regex_case_insensitive = FuzzyRegexBuilder::new("(?:quick){e<=1}")
        .case_insensitive(true)
        .build()
        .unwrap();
    let regex_case_sensitive = FuzzyRegexBuilder::new("(?:QUICK){e<=1}")
        .build()
        .unwrap();
    bench("Case insensitive 'quick' e<=1", 10_000, || regex_case_insensitive.find(text).is_some());
    bench("Case sensitive 'QUICK' e<=1", 10_000, || regex_case_sensitive.find(text).is_some());
}

fn bench_real_world_patterns() {
    println!("\n--- 9. Real-World Patterns ---");

    // Email-like pattern
    let email_text = "Contact us at support@example.com for more information.";
    let regex_email = FuzzyRegexBuilder::new("(?:support){e<=2}")
        .build()
        .unwrap();
    bench("Email prefix 'support' e<=2", 10_000, || regex_email.find(email_text).is_some());

    // Name matching (fuzzy name search)
    let names = "John Smith, Jane Doe, Robert Johnson, Michael Williams, David Brown";
    let regex_name = FuzzyRegexBuilder::new("(?:Johnson){e<=2}")
        .build()
        .unwrap();
    bench("Name 'Johnson' e<=2", 10_000, || regex_name.find(names).is_some());

    // Address matching
    let address = "123 Main Street, Springfield, IL 62701";
    let regex_address = FuzzyRegexBuilder::new("(?:Springfield){e<=2}")
        .build()
        .unwrap();
    bench("City 'Springfield' e<=2", 10_000, || regex_address.find(address).is_some());

    // Product code
    let products = "SKU: ABC-12345-XYZ, Price: $99.99, Stock: 150 units";
    let regex_sku = FuzzyRegexBuilder::new("(?:ABC-12345){e<=1}")
        .build()
        .unwrap();
    bench("SKU 'ABC-12345' e<=1", 10_000, || regex_sku.find(products).is_some());
}

fn bench_unicode_international() {
    println!("\n--- 10. Unicode/International ---");
    let unicode_text = "Привет мир! Hello world! 你好世界！";
    let regex_russian = FuzzyRegexBuilder::new("(?:Привет){e<=1}")
        .build()
        .unwrap();
    let regex_chinese = FuzzyRegexBuilder::new("(?:你好){e<=1}")
        .build()
        .unwrap();
    bench("Russian 'Привет' e<=1", 10_000, || regex_russian.find(unicode_text).is_some());
    bench("Chinese '你好' e<=1", 10_000, || regex_chinese.find(unicode_text).is_some());
}

fn bench_exact_vs_fuzzy() {
    println!("\n--- 11. Exact vs Fuzzy Match ---");
    let text = "The quick brown fox jumps over the lazy dog.";
    for edit_dist in [0, 1, 2, 3] {
        let pattern = format!("(?:quick){{e<={edit_dist}}}");
        let regex = FuzzyRegexBuilder::new(&pattern)
            .build()
            .unwrap();
        bench(&format!("Exact text, e<={edit_dist}"), 10_000, || regex.find(text).is_some());
    }
}

fn bench_multiple_matches() {
    println!("\n--- 12. Multiple Matches ---");
    let text_repeat = "cat bat rat cat bat rat cat bat rat ".repeat(10);
    let regex_multi = FuzzyRegexBuilder::new("(?:cat|bat|rat){e<=1}")
        .build()
        .unwrap();
    bench("Count all matches (cat|bat|rat)", 1_000, || {
        regex_multi.find_iter(&text_repeat).count() > 0
    });
}

fn main() {
    println!("=== fuzzy-regex Comprehensive Benchmark ===\n");

    bench_text_length_scaling();
    bench_match_position_impact();
    bench_edit_distance_scaling();
    bench_pattern_length_scaling();
    bench_dna_bioinformatics();
    bench_dna_size_scaling();
    bench_no_match();
    bench_alternation_patterns();
    bench_case_insensitive();
    bench_real_world_patterns();
    bench_unicode_international();
    bench_exact_vs_fuzzy();
    bench_multiple_matches();

    println!("\n=== Benchmark Complete ===");
}
