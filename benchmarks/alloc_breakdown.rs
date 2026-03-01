//! Breakdown allocations by size to identify patterns

use fuzzy_regex::FuzzyRegexBuilder;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

// Track allocation sizes in buckets
static TINY: AtomicUsize = AtomicUsize::new(0);      // 1-32 bytes
static SMALL: AtomicUsize = AtomicUsize::new(0);     // 33-128 bytes
static MEDIUM: AtomicUsize = AtomicUsize::new(0);    // 129-512 bytes
static LARGE: AtomicUsize = AtomicUsize::new(0);     // 513-2048 bytes
static HUGE: AtomicUsize = AtomicUsize::new(0);      // 2049+ bytes
static TOTAL_BYTES: AtomicUsize = AtomicUsize::new(0);

struct TracingAlloc;

unsafe impl GlobalAlloc for TracingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { unsafe {
        let size = layout.size();
        TOTAL_BYTES.fetch_add(size, Ordering::Relaxed);
        match size {
            1..=32 => TINY.fetch_add(1, Ordering::Relaxed),
            33..=128 => SMALL.fetch_add(1, Ordering::Relaxed),
            129..=512 => MEDIUM.fetch_add(1, Ordering::Relaxed),
            513..=2048 => LARGE.fetch_add(1, Ordering::Relaxed),
            _ => HUGE.fetch_add(1, Ordering::Relaxed),
        };
        System.alloc(layout)
    }}

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe {
        System.dealloc(ptr, layout);
    }}
}

#[global_allocator]
static GLOBAL: TracingAlloc = TracingAlloc;

fn reset() {
    TINY.store(0, Ordering::Relaxed);
    SMALL.store(0, Ordering::Relaxed);
    MEDIUM.store(0, Ordering::Relaxed);
    LARGE.store(0, Ordering::Relaxed);
    HUGE.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
}

fn print_stats(label: &str) {
    let tiny = TINY.load(Ordering::Relaxed);
    let small = SMALL.load(Ordering::Relaxed);
    let medium = MEDIUM.load(Ordering::Relaxed);
    let large = LARGE.load(Ordering::Relaxed);
    let huge = HUGE.load(Ordering::Relaxed);
    let bytes = TOTAL_BYTES.load(Ordering::Relaxed);
    let total = tiny + small + medium + large + huge;

    println!("{label}:");
    println!("  Tiny (1-32b):      {tiny:3}");
    println!("  Small (33-128b):   {small:3}");
    println!("  Medium (129-512b): {medium:3}");
    println!("  Large (513-2048b): {large:3}");
    println!("  Huge (2049+b):     {huge:3}");
    println!("  Total: {total} allocs, {bytes} bytes");
}

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

    reset();
    let _ = fr.find(text);
    print_stats("Single find()");
}
