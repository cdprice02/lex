//! Benches for the entropy scan.

use divan::Bencher;
use std::hint::black_box;
use std::sync::Arc;

use super::counters::{Scratch, guess_entropy, guess_entropy_columns};
use super::{
    Allowed, Candidates, Code, Guess, Pool, Strategy, Word, WordCorrectness, synthetic_pool,
};
use crate::transposed::{Columns, Plan};

#[divan::bench]
fn guess_entropy_10_words(b: Bencher) {
    let pairs: Vec<(Word<5>, u64)> = (0..10_u64)
        .map(|i| (Word::from([crate::guesser::letter(i); 5]), 10 - i))
        .collect();
    let pool = Pool::new(&lex_data::WordSet::from_pairs(pairs));
    let guess = *pool.code(0);
    let words: Vec<Code<5>> = pool.indices().map(|i| *pool.code(i)).collect();
    let probs = vec![0.1; words.len()];
    let mut scratch = Scratch::take::<5>();
    b.bench_local(|| black_box(guess_entropy(&guess, &words, &probs, &mut scratch)));
}

/// 1000 candidates and 16 guesses over a..z, so repeated letters and overlaps come up as in a
/// real corpus; the old and new kernels score the same pairs.
fn entropy_fixture<const N: usize>() -> (Arc<Pool<N>>, Vec<Code<N>>, Vec<f64>) {
    let pool = synthetic_pool::<N>(1000);
    let words = pool.indices().map(|i| *pool.code(i)).collect();
    let probs = vec![1.0 / 1000.0; 1000];
    (pool, words, probs)
}

#[divan::bench(consts = [3, 5, 9, 15, 25])]
fn entropy_16_guesses_old<const N: usize>(b: Bencher) {
    let (pool, words, probs) = entropy_fixture::<N>();
    let mut scratch = Scratch::take::<N>();
    b.bench_local(|| {
        for i in 0..16 {
            black_box(guess_entropy(
                pool.code(i * 7),
                black_box(&words),
                &probs,
                &mut scratch,
            ));
        }
    });
}

#[divan::bench(consts = [3, 5, 9, 15, 25])]
fn entropy_16_guesses_new<const N: usize>(b: Bencher) {
    let (pool, words, probs) = entropy_fixture::<N>();
    let columns = Columns::gather(words.iter(), pool.alphabet_len());
    let mut scratch = Scratch::take::<N>();
    b.bench_local(|| {
        for i in 0..16 {
            black_box(guess_entropy_columns(
                &Plan::new(pool.code(i * 7)),
                black_box(&columns),
                &probs,
                &mut scratch,
            ));
        }
    });
}

#[divan::bench(consts = [3, 5, 9, 15, 25])]
fn gather_1000<const N: usize>(b: Bencher) {
    let (pool, words, _) = entropy_fixture::<N>();
    b.bench(|| {
        black_box(Columns::gather(
            black_box(&words).iter(),
            pool.alphabet_len(),
        ))
    });
}

fn bench_suggest<const N: usize>(b: Bencher, count: usize) {
    let candidates = Candidates::whole_pool(synthetic_pool::<N>(count));
    let strategy = Strategy::default();
    b.bench(|| black_box(candidates.suggest(&strategy, &Allowed::Whole)));
}

#[divan::bench]
fn root_suggest_all_1000_n5(b: Bencher) {
    bench_suggest::<5>(b, 1000);
}

#[divan::bench]
fn root_suggest_all_300_n10(b: Bencher) {
    bench_suggest::<10>(b, 300);
}

/// Narrows a 3000-word pool with its most frequent words as guesses, as a game would, until at
/// most `limit` candidates remain, then times a full-pool `suggest`. Most nodes of a full run
/// are deep ones like this: few candidates scored against a large pool.
fn bench_deep_suggest(b: Bencher, limit: usize) {
    let mut candidates = Candidates::whole_pool(synthetic_pool::<5>(3000));
    let target = candidates.pool.word(1);
    for i in 2..400 {
        if candidates.len() <= limit {
            break;
        }
        let word = candidates.pool.word(i);
        let feedback = WordCorrectness::correct(target, word);
        candidates = candidates.filtered(&Guess::new(word, feedback));
    }
    assert!(
        (2..=limit).contains(&candidates.len()),
        "{}",
        candidates.len()
    );
    let strategy = Strategy::default();
    b.bench(|| black_box(candidates.suggest(&strategy, &Allowed::Whole)));
}

#[divan::bench]
fn deep_suggest_all_60_of_3000_n5(b: Bencher) {
    bench_deep_suggest(b, 60);
}

#[divan::bench]
fn deep_suggest_all_8_of_3000_n5(b: Bencher) {
    bench_deep_suggest(b, 8);
}

#[divan::bench]
fn filtered_1000_n5(b: Bencher) {
    let candidates = Candidates::whole_pool(synthetic_pool::<5>(1000));
    let target = candidates.pool.word(0);
    let guess = Guess::new(
        candidates.pool.word(1),
        WordCorrectness::correct(target, candidates.pool.word(1)),
    );
    b.bench(|| black_box(candidates.filtered(&guess)));
}
