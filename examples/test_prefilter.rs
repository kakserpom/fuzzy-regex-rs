//! Debug exact match issue
use fuzzy_regex::FuzzyRegex;

fn main() {
    // Test exact match with fuzzy pattern
    println!("=== Test hello~2 matching 'hello' ===");
    let re = FuzzyRegex::new("hello~2").unwrap();
    let m = re.find("hello");
    println!("Match: {:?}", m);
    if let Some(m) = m {
        println!("Edits: total={}, i={}, d={}, s={}",
            m.edits().total(),
            m.edits().insertions,
            m.edits().deletions,
            m.edits().substitutions
        );
        println!("Similarity: {}", m.similarity());
    }
}
