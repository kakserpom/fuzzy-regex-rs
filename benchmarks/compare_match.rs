use fuzzy_regex::FuzzyRegexBuilder;

fn main() {
    let text = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";
    
    // k=2
    let fr2 = FuzzyRegexBuilder::new("(?:saddam)~2")
        .similarity(0.6)
        .build()
        .unwrap();
    let m2 = fr2.find(text);
    println!("fuzzy-regex k=2: match={:?}", m2.map(|m| (m.start(), m.end(), m.as_str())));
    
    // k=4
    let fr4 = FuzzyRegexBuilder::new("(?:saddam)~4")
        .similarity(0.3)
        .build()
        .unwrap();
    let m4 = fr4.find(text);
    println!("fuzzy-regex k=4: match={:?}", m4.map(|m| (m.start(), m.end(), m.as_str())));
}
