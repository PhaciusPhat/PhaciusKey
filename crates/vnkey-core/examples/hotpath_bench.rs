//! Temporary measurement harness: counts heap allocations and time per keystroke
//! through `Engine::process`, the keyboard-hook path.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use vnkey_core::{Config, Engine, Keystroke};

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static A: Counting = Counting;

// A paragraph of ordinary Telex typing: Vietnamese words, English words that must
// auto-restore, tone keys, and boundaries.
const CORPUS: &str = "Tooi laf ngwowif Vieejt Nam. Hoom nay trowfi ddepj quas, \
toi muoons ddi choi vowis banj be. Chungs ta seex gawpj nhau ows quans cafe \
gaanf nhaf ga. Neus banj rangr, hayx gooij dienj thoaij cho toi. \
The quick brown fox jumps over the lazy dog. Testing reset commit hello world. \
Xin chaof cacs banj, chucs mowfng nawm mowis vaf hanhj phucs. ";

fn main() {
    let mut engine = Engine::new(Config::default());

    // Warm up so the measurement excludes first-touch effects.
    for ch in CORPUS.chars() {
        std::hint::black_box(engine.process(Keystroke::char(ch)));
    }

    const ROUNDS: usize = 200;
    let keystrokes = CORPUS.chars().count() * ROUNDS;

    let allocs_before = ALLOCS.load(Ordering::Relaxed);
    let bytes_before = BYTES.load(Ordering::Relaxed);
    let start = Instant::now();

    for _ in 0..ROUNDS {
        for ch in CORPUS.chars() {
            std::hint::black_box(engine.process(Keystroke::char(ch)));
        }
    }

    let elapsed = start.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs_before;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes_before;

    println!("keystrokes      {keystrokes}");
    println!(
        "allocations     {allocs}  ({:.2} per keystroke)",
        allocs as f64 / keystrokes as f64
    );
    println!(
        "bytes allocated {bytes}  ({:.1} per keystroke)",
        bytes as f64 / keystrokes as f64
    );
    println!(
        "time            {:?}  ({:.0} ns per keystroke)",
        elapsed,
        elapsed.as_nanos() as f64 / keystrokes as f64
    );
}
