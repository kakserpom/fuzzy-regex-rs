//! Count prefilter candidates to understand performance difference

fn count_byte(bytes: &[u8], target: u8) -> usize {
    let mut count = 0;
    for &byte in bytes {
        if byte == target {
            count += 1;
        }
    }
    count
}

fn main() {
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum eros ipsum, tincidutn eu metus ut";
    let text_bytes = text.as_bytes();

    println!("Text length: {} bytes", text.len());
    println!("'tincidutn' at byte position: {:?}\n", text.find("tincidutn"));

    // Check what prefilter.fuzzy does for case-sensitive
    // Pattern "tincidunt" with 1 edit: search_depth = 2, collects bytes from pos 0-1
    // Position 0: 't' -> adds 't', 'T'
    // Position 1: 'i' -> adds 'i', 'I' (if unique)
    // Result: ThreeBytes { 't', 'T', 'i', max_offset: 1 }

    // For case-insensitive: only searches for first char's case variants
    // Position 0: 't' -> TwoBytes { 't', 'T', max_offset: 0 }

    // Count occurrences in text
    let t_lower = count_byte(text_bytes, b't');
    let t_upper = count_byte(text_bytes, b'T');
    let i_lower = count_byte(text_bytes, b'i');
    let i_upper = count_byte(text_bytes, b'I');

    println!("Character counts in text:");
    println!("  't' (lowercase): {t_lower}");
    println!("  'T' (uppercase): {t_upper}");
    println!("  'i' (lowercase): {i_lower}");
    println!("  'I' (uppercase): {i_upper}");

    println!("\nPrefilter analysis:");
    println!("  CI prefilter: TwoBytes('t', 'T'), max_offset=0");
    println!("    -> finds {} candidates (t + T)", t_lower + t_upper);

    println!("  CS fuzzy prefilter: ThreeBytes('t', 'T', 'i'), max_offset=1");
    println!("    -> finds {} candidates (t + T + i), each expanded by offset", t_lower + t_upper + i_lower);
    println!("    -> effective candidates: {} * 2 = {} positions", t_lower + t_upper + i_lower, (t_lower + t_upper + i_lower) * 2);

    // The actual difference:
    // CI: searches ~5 positions
    // CS: searches ~24 positions (12 * 2 with offset expansion)
    // This could explain the 3x slowdown!
}
