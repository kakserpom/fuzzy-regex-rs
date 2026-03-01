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
    
    // Time individual find_at calls
    let positions = [0, 17, 112, 229, 460]; // Different starting positions
    for &pos in &positions {
        let start = Instant::now();
        let result = re.find_at(&text, pos);
        let elapsed = start.elapsed().as_micros();
        println!("find_at(text, {}) = {:?} in {}µs", 
            pos, 
            result.map(|m| (m.start(), m.end())), 
            elapsed);
    }
    
    // Simulate find_iter
    println!("\nSimulating find_iter:");
    let mut pos = 0;
    let mut matches = 0;
    let total_start = Instant::now();
    while pos <= text.len() {
        let start = Instant::now();
        let result = re.find_at(&text, pos);
        let elapsed = start.elapsed().as_micros();
        
        if let Some(m) = result {
            if !(3..38).contains(&matches) {
                println!("  find_at({}) -> [{}, {}] in {}µs", pos, m.start(), m.end(), elapsed);
            } else if matches == 3 {
                println!("  ...");
            }
            matches += 1;
            pos = m.end();
        } else {
            break;
        }
    }
    let total = total_start.elapsed().as_micros();
    #[allow(clippy::cast_precision_loss)]
    let avg = total as f64 / f64::from(matches);
    println!("Total: {matches} matches in {total}µs ({avg:.1}µs per match)");
}
