//! Prints the Opening book's node count, in-memory size and build time on real data. Opt-in:
//! `LEX_DATA_DIR=<data dir> cargo test -p lex-core --release --test book_measure -- --ignored --nocapture`
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

use std::time::Instant;

use lex_core::{BOOK_FLOOR, Guesser, Strategy};
use lex_data::{DataDir, Language, Profile};

fn report<const N: usize>(track: &str, guesser: &Guesser<N>) {
    let started = Instant::now();
    let book = guesser.build_book();
    let elapsed = started.elapsed();
    println!(
        "{track}: floor {BOOK_FLOOR}, {} nodes, {} bytes in memory, build {elapsed:?}",
        book.len(),
        book.heap_bytes()
    );
}

#[test]
#[ignore = "needs the English-5 cache and the Wordle profile in data/"]
fn book_size_and_build_time() {
    let dir = DataDir::new(std::env::var("LEX_DATA_DIR").unwrap_or_else(|_| "../data".into()));
    let strategy = Strategy::default();

    let pool = dir.load::<5>(Language::English, None).unwrap();
    let answers: Vec<_> = pool.words().collect();
    report("english-5", &Guesser::new(&pool, &answers, strategy));

    let data = dir.load_profile::<5>(Profile::Wordle).unwrap();
    report(
        "wordle profile",
        &Guesser::new(&data.pool, &data.answers, strategy),
    );
}
