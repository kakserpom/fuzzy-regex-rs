#![allow(clippy::cast_precision_loss)]

use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris \
        nisi ut aliquip ex ea commodo consequat.";
    let text = lorem.repeat(20);
    
    println!("Text length: {} chars", text.len());
    
    let re = FuzzyRegex::new(".*?dolor").unwrap();
    
    // Time find() - single match
    let start = Instant::now();
    for _ in 0..100 {
        let _ = re.find(&text);
    }
    let find_time = start.elapsed().as_micros() as f64 / 100.0;
    println!("find() single: {find_time:.1}µs");
    
    // Time find_iter() - all matches
    let start = Instant::now();
    for _ in 0..10 {
        let count = re.find_iter(&text).count();
        if count != 40 {
            println!("Unexpected count: {count}");
        }
    }
    let iter_time = start.elapsed().as_micros() as f64 / 10.0;
    println!("find_iter() all: {:.1}µs ({:.1}µs per match)", iter_time, iter_time / 40.0);
    
    // Compare with simpler patterns
    let re_simple = FuzzyRegex::new("dolor").unwrap();
    let start = Instant::now();
    for _ in 0..10 {
        let _ = re_simple.find_iter(&text).count();
    }
    let simple_time = start.elapsed().as_micros() as f64 / 10.0;
    println!("'dolor' find_iter(): {simple_time:.1}µs");
}
