//! Count allocations during `find()`

use fuzzy_regex::FuzzyRegexBuilder;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { unsafe {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }}

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe {
        System.dealloc(ptr, layout);
    }}
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn main() {
    let text = "xxxx xxxx xxxx xxxx xxxx xxxx saddam";

    let fr = FuzzyRegexBuilder::new("(?:saddam)~2")
        .similarity(0.6)
        .build()
        .unwrap();

    // Warmup
    for _ in 0..10 {
        let _ = fr.find(text);
    }

    // Reset counters
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);

    // Single find
    let _ = fr.find(text);

    let count = ALLOC_COUNT.load(Ordering::Relaxed);
    let bytes = ALLOC_BYTES.load(Ordering::Relaxed);

    println!("Single find():");
    println!("  Allocations: {count}");
    println!("  Bytes: {bytes}");

    // Reset and do 100 finds
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);

    for _ in 0..100 {
        let _ = fr.find(text);
    }

    let count = ALLOC_COUNT.load(Ordering::Relaxed);
    let bytes = ALLOC_BYTES.load(Ordering::Relaxed);

    println!("\n100 find() calls:");
    println!("  Allocations: {} ({} per call)", count, count / 100);
    println!("  Bytes: {} ({} per call)", bytes, bytes / 100);
}
