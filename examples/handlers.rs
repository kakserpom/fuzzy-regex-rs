//! Example demonstrating custom handlers with `(?call:name)` syntax.

use fuzzy_regex::{FuzzyRegexBuilder, HandlerResult};

fn main() {
    println!("=== Handler Examples ===\n");

    example_override_capture();
    example_fuzzy_lookbehind_with_handler();
}

fn example_override_capture() {
    println!("Example: Override capture text");
    println!("------------------------------");

    let re = FuzzyRegexBuilder::new(r"(prefix(?call:handler)suffix)")
        .handler("handler", |text, pos| {
            if text[pos..].starts_with("XYZ") {
                HandlerResult::MatchOverride(3, "xyz".to_string())
            } else {
                HandlerResult::NoMatch
            }
        })
        .build()
        .unwrap();

    let test = "prefixXYZsuffix";
    if let Some(caps) = re.captures(test) {
        println!("  Input:  {:?}", test);
        println!("  Group 0 (full): {:?}", caps.get(0).unwrap().as_str());
        println!("  Group 1 (capture): {:?}", caps.get(1).unwrap().as_str());
    }
    println!();
}

fn example_fuzzy_lookbehind_with_handler() {
    println!("Example: Handler override with fuzzy lookbehind");
    println!("---------------------------------------------");

    // Debug: just the quoted string part first
    println!("Test 1: Just quoted string with handler");
    let re1 = FuzzyRegexBuilder::new(r#""(?call:translate)""#)
        .handler("translate", |text, pos| {
            let remaining = &text[pos..];
            eprintln!(
                "  DEBUG handler at pos={}, remaining={:?} (len={})",
                pos,
                remaining,
                remaining.len()
            );
            // "привет" is 6 Cyrillic chars = 12 bytes in UTF-8
            let starts = remaining.starts_with("привет");
            eprintln!("    starts_with('привет') = {}", starts);
            if starts {
                HandlerResult::MatchOverride(12, "HELLO".to_string()) // 12 bytes, not 6 chars
            } else {
                HandlerResult::NoMatch
            }
        })
        .build()
        .unwrap();

    let test1 = "\"привет\"";
    eprintln!("  Input: {:?} (len={})", test1, test1.len());
    for (i, c) in test1.char_indices() {
        eprintln!("    pos {}: {:?}", i, c);
    }
    let m = re1.find(test1);
    eprintln!("  Result: {:?}", m.as_ref().map(|x| x.as_str().to_string()));
    println!("  find() => {:?}", m.map(|x| x.as_str().to_string()));

    // Check captures for override
    if let Some(caps) = re1.captures(test1) {
        println!("  Captures:");
        for i in 0..caps.len() {
            if let Some(m) = caps.get(i) {
                println!("    Group {}: {:?}", i, m.as_str());
            }
        }
        println!("  Handler overrides: {:?}", caps.handler_overrides());
    }

    // Debug: lookbehind alone
    println!("\nTest 2: Just lookbehind");
    let re2 = FuzzyRegexBuilder::new(r"(?<=(?:hello){e<=2}) world")
        .build()
        .unwrap();

    for test in [
        "hello world",
        "helllo world",
        "hellllo world",
        "helllllo world",
    ] {
        let m = re2.find(test);
        println!("  {:?} => {:?}", test, m.map(|x| x.as_str().to_string()));
    }

    // Full pattern
    println!("\nTest 3: Full pattern with handler + lookbehind");
    let re3 = FuzzyRegexBuilder::new(r#"(?<=(?:helllo){e<=2}) "(?call:translate)""#)
        .handler("translate", |text, pos| {
            let remaining = &text[pos..];
            // "привет" is 6 Cyrillic chars = 12 bytes in UTF-8
            if remaining.starts_with("привет") {
                HandlerResult::MatchOverride(12, "HELLO".to_string())
            } else {
                HandlerResult::NoMatch
            }
        })
        .build()
        .unwrap();

    for (test, desc) in [
        ("hello \"привет\"", "exact"),
        ("helllo \"привет\"", "1 extra l"),
    ] {
        let m = re3.find(test);
        println!(
            "  {} {} => {:?}",
            desc,
            test,
            m.map(|x| x.as_str().to_string())
        );

        if let Some(caps) = re3.captures(test) {
            println!("    Captures:");
            for i in 0..caps.len() {
                if let Some(m) = caps.get(i) {
                    println!("      Group {}: {:?}", i, m.as_str());
                }
            }
            println!("    Handler overrides: {:?}", caps.handler_overrides());
        }
    }
    println!();
}
