//! Benches for the Guesser.

use divan::Bencher;
use std::collections::HashMap;
use std::hint::black_box;

use lex_data::{Word, WordSet};

use super::{Guess, Guesser, Strategy, WordCorrectness, corpus_guesser};

fn make_wordset(n: u64) -> Vec<(Word<5>, u64)> {
    (0..n)
        .map(|i| {
            let word = Word::from([
                crate::guesser::letter(i),
                crate::guesser::letter(i / 26),
                crate::guesser::letter(i / 676),
                crate::guesser::letter(i / 17576),
                crate::guesser::letter(i / 456_976),
            ]);
            (word, n - i)
        })
        .collect()
}

fn build_guesser(pairs: &[(Word<5>, u64)]) -> Guesser<5> {
    let frequencies: HashMap<Word<5>, u64> = pairs.iter().copied().collect();
    let ws = WordSet::from_frequency_map(frequencies);
    corpus_guesser(&ws, Strategy::ultrahard())
}

#[divan::bench]
fn suggest_50_word_dict(b: Bencher) {
    let pairs = make_wordset(50);
    let first_guess = pairs[0].0;
    let target = pairs[1].0;
    let initial_guess = Guess::new(first_guess, WordCorrectness::correct(target, first_guess));
    b.bench(|| {
        let mut g = build_guesser(&pairs);
        g.push_guess(initial_guess.clone());
        black_box(g.suggest())
    });
}

#[divan::bench]
fn suggest_200_word_dict(b: Bencher) {
    let pairs = make_wordset(200);
    let first_guess = pairs[0].0;
    let target = pairs[1].0;
    let initial_guess = Guess::new(first_guess, WordCorrectness::correct(target, first_guess));
    b.bench(|| {
        let mut g = build_guesser(&pairs);
        g.push_guess(initial_guess.clone());
        black_box(g.suggest())
    });
}
