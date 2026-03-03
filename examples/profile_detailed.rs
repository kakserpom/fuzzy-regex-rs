use fuzzy_regex::FuzzyRegex;
use std::time::Instant;

fn main() {
    let short = "The quick brown fox jumps over the lazy dog.";
    let long = short.repeat(100);

    let tests = [
        ("quick", "exact"),
        (r"\d+", "digit"),
        ("(?:quick){2}", "repeat"),
        ("(?:quick|brown|fox)", "alt"),
        ("[a-z]+", "class"),
    ];

    println!("{:20} {:>12} {:>12}", "pattern", "short", "long");
    println!("{}", "-".repeat(50));

    for (pat, name) in tests {
        let re = FuzzyRegex::new(pat).unwrap();

        // Warmup
        for _ in 0..100 {
            re.find(short);
        }
        for _ in 0..10 {
            re.find(&long);
        }

        // Short
        let start = Instant::now();
        for _ in 0..10000 {
            re.find(short);
        }
        let short_ns = start.elapsed().as_nanos() as f64 / 10000.0;

        // Long
        let start = Instant::now();
        for _ in 0..100 {
            re.find(&long);
        }
        let long_ns = start.elapsed().as_nanos() as f64 / 100.0;

        println!("{:20} {:12.1} {:12.1}", name, short_ns, long_ns);
    }
}
