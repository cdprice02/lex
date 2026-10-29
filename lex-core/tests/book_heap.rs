//! The Opening book's heap against the 1 GB cap. This binary owns the global allocator, so the
//! other test binaries are unaffected. The fixture test runs everywhere; the English-5 test is
//! opt-in because it builds the real book:
//! `LEX_DATA_DIR=<data dir> cargo test -p lex-core --release --test book_heap -- --ignored --nocapture`
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

use std::sync::Mutex;

use lex_core::{Guesser, OpeningBook, Strategy};
use lex_data::{DataDir, Language, Word, WordSet};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// The cap on the book's heap, in bytes.
const CAP: u64 = 1 << 30;

/// One dhat profiler may exist at a time.
static PROFILER: Mutex<()> = Mutex::new(());

const FIXTURE: &str = include_str!("fixtures/words5.txt");

fn fixture() -> WordSet<5> {
    WordSet::from_pairs(
        FIXTURE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let (word, freq) = l.split_once(' ').expect("`word frequency` line");
                (Word::<5>::try_from(word).unwrap(), freq.parse().unwrap())
            })
            .collect(),
    )
}

/// Builds a book with `build`, round-trips it through its stored form, and checks the heap:
/// dropping the loaded book must free what `heap_bytes` claims, and the peak over the whole
/// build and load must be under the cap. Returns (peak bytes, nodes).
fn measure<const N: usize>(
    guesser: &Guesser<N>,
    pool_len: usize,
    build: impl FnOnce(&Guesser<N>) -> OpeningBook<N>,
) -> (u64, usize) {
    let _guard = PROFILER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _profiler = dhat::Profiler::builder().testing().build();
    let fresh = build(guesser);
    let stored = fresh.to_nodes();
    drop(fresh);
    let loaded = OpeningBook::<N>::from_nodes(stored, pool_len).unwrap();
    let peak = dhat::HeapStats::get().max_bytes as u64;
    let (nodes, claimed) = (loaded.len(), loaded.heap_bytes());
    let live = dhat::HeapStats::get().curr_bytes;
    drop(loaded);
    let freed = live - dhat::HeapStats::get().curr_bytes;
    println!("{nodes} nodes, {claimed} bytes claimed in memory, peak heap {peak} bytes of {CAP}");
    // `heap_bytes` also counts the struct itself, which lives on the stack here.
    assert_eq!(
        freed,
        claimed - std::mem::size_of::<OpeningBook<N>>(),
        "heap_bytes disagrees with the allocator"
    );
    assert!(peak < CAP, "peak heap passes the cap");
    (peak, nodes)
}

#[test]
fn a_fixture_book_stays_far_under_the_cap() {
    let pool = fixture();
    let pool_len = pool.len();
    let answers: Vec<_> = pool.words().collect();
    let guesser = Guesser::new(&pool, &answers, Strategy::default());
    let (peak, nodes) = measure(&guesser, pool_len, |g| g.build_book_with_floor(1));
    assert!(nodes > 0);
    assert!(peak < CAP / 100);
}

#[test]
#[ignore = "builds the real English-5 book; needs the cache in data/"]
fn the_english_5_book_fits_the_cap() {
    let dir = DataDir::new(std::env::var("LEX_DATA_DIR").unwrap_or_else(|_| "../data".into()));
    let pool = dir.load::<5>(Language::English, None).unwrap();
    let pool_len = pool.len();
    let answers: Vec<_> = pool.words().collect();
    let guesser = Guesser::new(&pool, &answers, Strategy::default());
    measure(&guesser, pool_len, lex_core::Guesser::build_book);
}
