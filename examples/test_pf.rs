use fuzzy_regex::FuzzyRegex;

fn main() {
    // Check prefilter for "hello" with 2 edits
    let re = FuzzyRegex::new("(?:hello){e<=2}").unwrap();
    println!("{:#?}", re);
}
