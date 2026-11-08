//! Tests for the Candidate set, the entropy scan and its counters.

use super::*;

#[test]
fn entropy_buffer_serves_every_word_length_on_one_thread() {
    let five: Code<5> = [0, 1, 2, 3, 4];
    let ten: Code<10> = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    // N=5 sizes the shared buffer first; N=10 must grow it rather than index past the end
    let mut scratch = Scratch::take::<5>();
    assert_eq!(
        guess_entropy(&five, &[five], &[1.0], &mut scratch)
            .abs()
            .to_bits(),
        0
    );
    scratch.give_back();
    let mut scratch = Scratch::take::<10>();
    assert_eq!(
        guess_entropy(&ten, &[ten], &[1.0], &mut scratch)
            .abs()
            .to_bits(),
        0
    );
    scratch.give_back();
    let mut scratch = Scratch::take::<5>();
    assert_eq!(
        guess_entropy(&five, &[five], &[1.0], &mut scratch)
            .abs()
            .to_bits(),
        0
    );
    scratch.give_back();
}

#[test]
fn filtered_by_a_guess_with_letters_outside_the_alphabet() {
    let pairs = [("crane", 50), ("stare", 40), ("light", 30), ("shore", 20)]
        .map(|(s, f)| (Word::<5>::try_from(s).unwrap(), f));
    let candidates =
        Candidates::whole_pool(Pool::new(&lex_data::WordSet::from_pairs(pairs.to_vec())));
    // z, q and x are in no pool word
    let guess = Word::<5>::try_from("zqxre").unwrap();
    let target = candidates.pool.word(0);
    let kept = candidates.filtered(&Guess::new(guess, WordCorrectness::correct(target, guess)));
    let expect: Vec<u32> = (0..4)
        .filter(|&i| {
            WordCorrectness::correct(candidates.pool.word(i), guess)
                == WordCorrectness::correct(target, guess)
        })
        .collect();
    assert_eq!(kept.indices(), expect);
    assert!(kept.indices().contains(&0));
}

/// Forty distinct words over a small alphabet, frequency-descending, so scores collide and
/// the tie-break order matters.
fn top_k_pool() -> Arc<Pool<5>> {
    let letters = ['a', 'b', 'c', 'd'];
    let mut seen = std::collections::HashSet::new();
    let pairs: Vec<(Word<5>, u64)> = (0..400_u64)
        .map(|i| -> [char; 5] {
            std::array::from_fn(|p| {
                letters[usize::try_from(i * 7 / 3 + i * i).unwrap_or(0) >> (2 * p) & 3]
            })
        })
        .filter(|chars| seen.insert(*chars))
        .take(40)
        .zip(0_u64..)
        .map(|(chars, i)| (Word::from(chars), 400 - i))
        .collect();
    assert_eq!(
        pairs.len(),
        40,
        "the generator yields enough distinct words"
    );
    Pool::new(&lex_data::WordSet::from_pairs(pairs))
}

#[test]
fn top_k_matches_the_full_sort_and_leads_with_suggest() {
    let pool = top_k_pool();
    let all: Vec<u32> = pool.indices().collect();
    let strategy = crate::strategy::Strategy::default();
    let sets = [
        all.iter().copied().step_by(2).collect::<Vec<_>>(),
        all.iter().copied().skip(5).take(9).collect::<Vec<_>>(),
        all.clone(),
    ];
    for set in sets {
        let candidates = Candidates::from_indices(Arc::clone(&pool), set.clone());
        let list = Allowed::List(
            all.iter()
                .copied()
                .step_by(3)
                .chain(set.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
        );
        for allowed in [Allowed::Whole, Allowed::Candidates, list] {
            // the reference scores every guess, then sorts all of them with a stable sort
            let mut full = Vec::new();
            candidates.for_each_guess(&allowed, strategy.temperature, |index, e, p, c| {
                full.push(Scored {
                    index,
                    entropy_bits: e,
                    p_answer: p,
                    in_candidates: c,
                });
            });
            full.sort_by(|a, b| {
                cmp_scores(b.score(strategy.win_weight), a.score(strategy.win_weight))
                    .then(b.in_candidates.cmp(&a.in_candidates))
            });
            for k in [0, 1, 2, 7, full.len(), full.len() + 3] {
                let top = candidates.suggest_top_k(&strategy, &allowed, k);
                assert_eq!(top, full[..k.min(full.len())], "k {k}");
            }
            assert_eq!(
                candidates.suggest_top_k(&strategy, &allowed, 1)[0].index,
                candidates.suggest_index(&strategy, &allowed).unwrap()
            );
        }
    }
}

#[test]
fn the_scan_scores_every_guess_as_the_scalar_counter_does() {
    let pool = top_k_pool();
    let all: Vec<u32> = pool.indices().collect();
    let strategy = crate::strategy::Strategy::default();
    // a whole pool, a strided subset and a single word; blocks past one are the proptests'
    for set in [
        all.clone(),
        all.iter().copied().step_by(3).collect::<Vec<_>>(),
        all[..1].to_vec(),
    ] {
        let candidates = Candidates::from_indices(Arc::clone(&pool), set.clone());
        let probs = pool.prior(&set, strategy.temperature);
        let words: Vec<Code<5>> = set.iter().map(|&i| *pool.code(i)).collect();
        let mut scratch = Scratch::take::<5>();
        let mut seen = 0;
        candidates.for_each_guess(&Allowed::Whole, strategy.temperature, |i, entropy, _, _| {
            let expect = guess_entropy(pool.code(i), &words, &probs, &mut scratch);
            assert_eq!(entropy.to_bits(), expect.to_bits(), "guess {i}");
            seen += 1;
        });
        scratch.give_back();
        assert_eq!(seen, pool.len());
    }
}

use proptest::prelude::*;

/// Words over a small alphabet so patterns collide and runs have several members.
fn counters_agree<const N: usize>(
    guess: [u8; N],
    pairs: &[([u8; N], f64)],
) -> Result<(), TestCaseError> {
    let words: Vec<Code<N>> = pairs.iter().map(|&(w, _)| w).collect();
    let probs: Vec<f64> = pairs.iter().map(|&(_, p)| p).collect();
    let columns = Columns::gather(words.iter(), 255);
    let plan = Plan::new(&guess);
    let mut scratch = Scratch::take::<N>();
    // the sorting counter is the one every length can run; the dense one only where it fits
    let mut bits = vec![
        entropy_scalar(&guess, &words, &probs, false, &mut scratch).to_bits(),
        entropy_columns(&plan, &columns, &probs, false, &mut scratch).to_bits(),
    ];
    if is_dense::<N>() {
        bits.push(entropy_scalar(&guess, &words, &probs, true, &mut scratch).to_bits());
        bits.push(entropy_columns(&plan, &columns, &probs, true, &mut scratch).to_bits());
    }
    scratch.give_back();
    prop_assert!(bits.iter().all(|&b| b == bits[0]), "{bits:?}");
    Ok(())
}

proptest! {
    #[test]
    fn counters_agree_n17(
        guess in prop::array::uniform17(0u8..5),
        pairs in prop::collection::vec((prop::array::uniform17(0u8..5), 1e-6f64..1.0), 1..150),
    ) {
        counters_agree::<17>(guess, &pairs)?;
    }

    #[test]
    fn counters_agree_n25(
        guess in prop::array::uniform25(0u8..3),
        pairs in prop::collection::vec((prop::array::uniform25(0u8..3), 1e-6f64..1.0), 1..150),
    ) {
        counters_agree::<25>(guess, &pairs)?;
    }

    #[test]
    fn dense_and_sorted_agree_n3(
        guess in prop::array::uniform3(0u8..3),
        pairs in prop::collection::vec((prop::array::uniform3(0u8..3), 1e-6f64..1.0), 1..60),
    ) {
        counters_agree::<3>(guess, &pairs)?;
    }

    #[test]
    fn dense_and_sorted_agree_n5(
        guess in prop::array::uniform5(0u8..4),
        pairs in prop::collection::vec((prop::array::uniform5(0u8..4), 1e-6f64..1.0), 1..80),
    ) {
        counters_agree::<5>(guess, &pairs)?;
    }

    #[test]
    fn dense_and_sorted_agree_n13(
        guess in prop::array::uniform13(0u8..4),
        pairs in prop::collection::vec((prop::array::uniform13(0u8..4), 1e-6f64..1.0), 1..80),
    ) {
        counters_agree::<13>(guess, &pairs)?;
    }
}

#[test]
fn dense_bound_is_a_memory_rule() {
    const { assert!(WordCorrectness::<13>::COUNT * 8 <= DENSE_BOUND_BYTES) };
    const { assert!(WordCorrectness::<14>::COUNT * 8 > DENSE_BOUND_BYTES) };
}
