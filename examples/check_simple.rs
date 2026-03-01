//! Check if Russian pattern is detected as simple
use fuzzy_regex::FuzzyRegexBuilder;

fn main() {
    let re_ru = FuzzyRegexBuilder::new("(?:Привет){e<=1}").build().unwrap();

    let re_en = FuzzyRegexBuilder::new("(?:Hello){e<=1}").build().unwrap();

    // Check is_simple_fuzzy
    println!("Russian is_simple_fuzzy: {}", re_ru.is_simple_fuzzy());
    println!("English is_simple_fuzzy: {}", re_en.is_simple_fuzzy());

    // Test basic find
    let text_ru = "Привет мир!";
    let text_en = "Hello world!";

    println!("\nRussian find result: {:?}", re_ru.find(text_ru));
    println!("English find result: {:?}", re_en.find(text_en));
}
