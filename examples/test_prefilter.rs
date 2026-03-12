use fuzzy_regex::FuzzyRegex;

fn main() {
    // Check what happens internally
    let pattern = r"[\w.+-]+@[\w.-]+\.\w+";
    let re = FuzzyRegex::new(pattern).unwrap();

    // This text has no @
    let text_no_at = "This is a long text without any email addresses in it at all.";

    let m = re.find(text_no_at);
    println!("Match in no-@ text: {:?}", m);

    // This text has @
    let text_with_at = "Contact test@example.com for info.";
    let m = re.find(text_with_at);
    println!("Match in @ text: {:?}", m);
}
