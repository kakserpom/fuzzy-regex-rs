//! Verify streaming search returns result

use std::time::Instant;

/// Build byte masks for case-insensitive matching
fn build_case_insensitive_masks(pattern: &str) -> [u64; 128] {
    let mut masks = [!0u64; 128];
    for (i, ch) in pattern.to_lowercase().chars().enumerate() {
        let byte = ch as u8;
        masks[byte as usize] &= !(1u64 << i);
        masks[byte.to_ascii_uppercase() as usize] &= !(1u64 << i);
    }
    masks
}

/// Build byte masks for case-sensitive matching
fn build_case_sensitive_masks(pattern: &str) -> [u64; 128] {
    let mut masks = [!0u64; 128];
    for (i, ch) in pattern.chars().enumerate() {
        let byte = ch as u8;
        masks[byte as usize] &= !(1u64 << i);
    }
    masks
}

/// Result of a streaming search match
type MatchResult = Option<(usize, usize, usize, f32)>;

/// Perform case-insensitive streaming search
fn search_case_insensitive(
    text: &[u8],
    masks: &[u64; 128],
    pattern_len: usize,
    max_edits: usize,
    threshold: f32,
) -> MatchResult {
    let accept_mask = 1u64 << (pattern_len - 1);
    let mut r = [!0u64; 2];
    r[1] = r[0] >> 1;
    let mut start_bytes = [0usize; 2];

    for (pos, &byte) in text.iter().enumerate() {
        let lookup = byte.to_ascii_lowercase();
        let char_mask = masks[lookup as usize];

        let old_r0 = r[0];
        let old_r1 = r[1];

        r[0] = (old_r0 << 1) | char_mask;
        if r[0] == !0u64 {
            start_bytes[0] = pos + 1;
        }

        let insert = old_r0;
        let delete = r[0] >> 1;
        let substitute = old_r0 << 1;
        let match_d = (old_r1 << 1) | char_mask;
        r[1] = match_d & insert & delete & substitute;
        if r[1] == !0u64 {
            start_bytes[1] = pos + 1;
        }

        let end = pos + 1;
        for d in 0..=max_edits {
            if (r[d] & accept_mask) == 0 {
                let matched_len = end - start_bytes[d];
                // Quick sim check (values are small, so u8 conversion is safe)
                let d_u8 = u8::try_from(d).expect("edit distance too large");
                let max_len = u8::try_from(pattern_len.max(matched_len)).expect("length too large");
                let sim = 1.0 - f32::from(d_u8) / f32::from(max_len);
                if sim >= threshold {
                    return Some((start_bytes[d], end, d, sim));
                }
            }
        }
    }
    None
}

/// Perform case-sensitive streaming search
fn search_case_sensitive(
    text: &[u8],
    masks: &[u64; 128],
    pattern_len: usize,
    max_edits: usize,
    threshold: f32,
) -> MatchResult {
    let accept_mask = 1u64 << (pattern_len - 1);
    let mut r = [!0u64; 2];
    r[1] = r[0] >> 1;
    let mut start_bytes = [0usize; 2];

    for (pos, &byte) in text.iter().enumerate() {
        let char_mask = masks[byte as usize];

        let old_r0 = r[0];
        let old_r1 = r[1];

        r[0] = (old_r0 << 1) | char_mask;
        if r[0] == !0u64 {
            start_bytes[0] = pos + 1;
        }

        let insert = old_r0;
        let delete = r[0] >> 1;
        let substitute = old_r0 << 1;
        let match_d = (old_r1 << 1) | char_mask;
        r[1] = match_d & insert & delete & substitute;
        if r[1] == !0u64 {
            start_bytes[1] = pos + 1;
        }

        let end = pos + 1;
        for d in 0..=max_edits {
            if (r[d] & accept_mask) == 0 {
                let matched_len = end - start_bytes[d];
                // Values are small, so u8 conversion is safe
                let d_u8 = u8::try_from(d).expect("edit distance too large");
                let max_len = u8::try_from(pattern_len.max(matched_len)).expect("length too large");
                let sim = 1.0 - f32::from(d_u8) / f32::from(max_len);
                if sim >= threshold {
                    return Some((start_bytes[d], end, d, sim));
                }
            }
        }
    }
    None
}

/// Benchmark case-insensitive streaming search
fn benchmark_case_insensitive(
    text: &[u8],
    masks: &[u64; 128],
    pattern_len: usize,
    max_edits: usize,
    threshold: f32,
    iterations: u32,
) {
    println!("=== Case-Insensitive Streaming ===");
    let start = Instant::now();
    for iter in 0..iterations {
        let found_at = search_case_insensitive(text, masks, pattern_len, max_edits, threshold);
        if iter == 0 {
            println!("Found: {found_at:?}");
        }
    }
    println!("Time: {:?} per iter", start.elapsed() / iterations);
}

/// Benchmark case-sensitive streaming search
fn benchmark_case_sensitive(
    text: &[u8],
    masks: &[u64; 128],
    pattern_len: usize,
    max_edits: usize,
    threshold: f32,
    iterations: u32,
) {
    println!("\n=== Case-Sensitive Streaming ===");
    let start = Instant::now();
    for iter in 0..iterations {
        let found_at = search_case_sensitive(text, masks, pattern_len, max_edits, threshold);
        if iter == 0 {
            println!("Found: {found_at:?}");
        }
    }
    println!("Time: {:?} per iter", start.elapsed() / iterations);
}

fn main() {
    // Simulate what find_first_boyer_moore does
    let text = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut";
    let pattern = "tincidunt";
    let threshold = 0.8f32;
    let max_edits = 1usize;
    let iterations = 10_000u32;

    // Setup masks
    let masks_case_insensitive = build_case_insensitive_masks(pattern);
    let masks_case_sensitive = build_case_sensitive_masks(pattern);

    // Run benchmarks
    benchmark_case_insensitive(
        text,
        &masks_case_insensitive,
        pattern.len(),
        max_edits,
        threshold,
        iterations,
    );
    benchmark_case_sensitive(
        text,
        &masks_case_sensitive,
        pattern.len(),
        max_edits,
        threshold,
        iterations,
    );
}
