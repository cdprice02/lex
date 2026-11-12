//! Tests for the Endgame search and its pattern table.

use lex_data::{Word, WordSet};

use super::*;
use crate::correctness::pattern_ordinal;
use crate::pool_ops::shared;
use crate::strategy::Pool;
use crate::test_support::{Rng, random_candidates, random_pool, similar_candidates};

const FAMILY: [&str; 7] = [
    "batch", "catch", "hatch", "latch", "match", "patch", "watch",
];

fn pool_of(words: &[&str]) -> Arc<Pool<5>> {
    let pairs = words
        .iter()
        .zip(0_u64..)
        .map(|(s, i)| (Word::<5>::try_from(*s).unwrap(), 1000 - i))
        .collect();
    Pool::new(&WordSet::from_pairs(pairs))
}

/// The guess of `Endgame::best_index` as a word.
fn best_guess(
    endgame: &mut Endgame,
    pool: &Pool<5>,
    set: &[u32],
    allowed: &Allowed,
) -> Option<Word<5>> {
    endgame.best_index(set, allowed).map(|i| pool.word(i))
}

fn ordinal_of(pool: &Pool<5>, word: Word<5>, guess: Word<5>) -> u64 {
    pattern_ordinal(&pool.encode(&word), &pool.encode(&guess), &mut [0; 256])
}

fn indices_of(pool: &Pool<5>, words: &[&str]) -> Vec<u32> {
    let mut v: Vec<u32> = words
        .iter()
        .map(|w| {
            let w = Word::<5>::try_from(*w).unwrap();
            pool.indices().find(|&i| pool.word(i) == w).unwrap()
        })
        .collect();
    v.sort_unstable();
    v
}

/// Plays every answer in the set with the endgame's own guesses and returns the weighted sum
/// of the objective over the games: the number the search must have predicted as its cost.
fn simulated_cost(endgame: &mut Endgame, pool: &Pool<5>, set: &[u32]) -> f64 {
    let mut total = 0.0;
    for &answer in set {
        let target = pool.word(answer);
        let mut remaining = set.to_vec();
        let mut guesses = 0;
        loop {
            let guess = best_guess(endgame, pool, &remaining, &Allowed::Whole).unwrap();
            guesses += 1;
            if guess == target {
                break;
            }
            let feedback = ordinal_of(pool, target, guess);
            remaining.retain(|&i| ordinal_of(pool, pool.word(i), guess) == feedback);
        }
        let objective = endgame
            .growth
            .map_or_else(|| f64::from(guesses), |a| a.powi(guesses));
        total = endgame.weight(answer).mul_add(objective, total);
    }
    total
}

#[test]
fn predicted_cost_matches_simulated_cost() {
    let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
    let set = indices_of(&pool, &FAMILY);
    for aversion in [0.0, 0.3, 1.0] {
        let mut endgame = Endgame::new(shared(&pool), GuessPool::All, 5.0, aversion);
        let predicted = endgame.solve(&set, &Allowed::Whole).cost;
        let simulated = simulated_cost(&mut endgame, &pool, &set);
        assert!(
            (predicted - simulated).abs() < 1e-9 * predicted,
            "aversion {aversion}: {predicted} vs {simulated}"
        );
    }
}

#[test]
fn aversion_avoids_a_long_tail_that_expectation_accepts() {
    // one common word and several rare ones sharing a pattern: plain expectation guesses the
    // common word first and walks the rare ones, aversion probes to bound the worst case
    let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
    let set = indices_of(&pool, &FAMILY);
    let worst = |aversion: f64| {
        let mut endgame = Endgame::new(shared(&pool), GuessPool::All, 5.0, aversion);
        set.iter()
            .map(|&answer| {
                let target = pool.word(answer);
                let mut remaining = set.clone();
                let mut guesses = 0;
                loop {
                    let guess =
                        best_guess(&mut endgame, &pool, &remaining, &Allowed::Whole).unwrap();
                    guesses += 1;
                    if guess == target {
                        return guesses;
                    }
                    let feedback = ordinal_of(&pool, target, guess);
                    remaining.retain(|&i| ordinal_of(&pool, pool.word(i), guess) == feedback);
                }
            })
            .max()
            .unwrap()
    };
    assert!(worst(2.0) <= worst(0.0));
}

#[test]
fn a_probe_word_beats_guessing_candidates() {
    let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
    let set = indices_of(&pool, &FAMILY);
    let all = Endgame::new(shared(&pool), GuessPool::All, 5.0, 0.0)
        .expected_guesses(&set, &Allowed::Whole);
    let hard = Endgame::new(shared(&pool), GuessPool::Ultrahard, 5.0, 0.0)
        .expected_guesses(&set, &Allowed::Candidates);
    assert!(all < hard, "{all} !< {hard}");
}

#[test]
fn the_memo_never_reuses_a_cost_across_different_allowed_guesses() {
    let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
    let set = indices_of(&pool, &FAMILY);
    let restricted = Allowed::List(set.clone());
    let mut endgame = Endgame::new(shared(&pool), GuessPool::All, 5.0, 0.0);
    let limited = endgame.expected_guesses(&set, &restricted);
    let free = endgame.expected_guesses(&set, &Allowed::Whole);
    assert!(free < limited, "{free} !< {limited}");
    // asking again, in either order, returns each allowed set's own cost
    assert_eq!(
        endgame.expected_guesses(&set, &restricted).to_bits(),
        limited.to_bits()
    );
    assert_eq!(
        endgame.expected_guesses(&set, &Allowed::Whole).to_bits(),
        free.to_bits()
    );
    let fresh =
        Endgame::new(shared(&pool), GuessPool::All, 5.0, 0.0).expected_guesses(&set, &restricted);
    assert_eq!(limited.to_bits(), fresh.to_bits());
}

#[test]
fn a_lone_candidate_is_one_guess() {
    let pool = pool_of(&FAMILY);
    let mut endgame = Endgame::new(shared(&pool), GuessPool::All, 5.0, 0.0);
    assert_eq!(
        endgame.expected_guesses(&[3], &Allowed::Whole).to_bits(),
        1.0_f64.to_bits()
    );
    assert_eq!(
        best_guess(&mut endgame, &pool, &[3], &Allowed::Whole),
        Some(pool.word(3))
    );
    assert_eq!(best_guess(&mut endgame, &pool, &[], &Allowed::Whole), None);
}

#[test]
fn two_candidates_cost_one_and_a_half_guesses_when_equally_likely() {
    let pool = Pool::new(&WordSet::from_pairs(vec![
        (Word::<5>::try_from("crane").unwrap(), 10),
        (Word::<5>::try_from("crate").unwrap(), 10),
    ]));
    let mut endgame = Endgame::new(shared(&pool), GuessPool::All, 5.0, 0.0);
    assert!((endgame.expected_guesses(&[0, 1], &Allowed::Whole) - 1.5).abs() < 1e-12);
}

/// Solves `set` with the fast paths and with the full search, each from a cold memo, and
/// demands the same guess and the same cost bit for bit. Where the memo key is the set alone,
/// every set the fast memo holds must also be the full search's optimum.
fn assert_matches_reference(
    pool: &Arc<Pool<5>>,
    mode: GuessPool,
    temperature: f64,
    aversion: f64,
    set: &[u32],
    allowed: &Allowed,
) {
    let mut fast = Endgame::new(shared(pool), mode, temperature, aversion);
    let mut full = Endgame::new(shared(pool), mode, temperature, aversion);
    full.reference = true;
    let (f, r) = (fast.solve(set, allowed), full.solve(set, allowed));
    let context = format!("{mode:?} T{temperature} L{aversion} {set:?} {allowed:?}");
    assert_eq!(f.guess, r.guess, "guess for {context}");
    assert_eq!(f.cost.to_bits(), r.cost.to_bits(), "cost for {context}");
    let implied = matches!(
        (mode, allowed),
        (GuessPool::All, Allowed::Whole) | (GuessPool::Ultrahard, Allowed::Candidates)
    );
    if implied {
        let held: Vec<(Vec<u32>, Solved)> = fast.memo[&None]
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        for (key, kept) in held {
            let exact = full.solve(&key, allowed);
            assert_eq!(kept.guess, exact.guess, "memo guess for {key:?}, {context}");
            assert_eq!(
                kept.cost.to_bits(),
                exact.cost.to_bits(),
                "memo cost for {key:?}, {context}"
            );
        }
    }
}

#[test]
fn the_fast_paths_match_the_full_search_bit_for_bit() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..90 {
        let size = 24 + rng.below(if round % 2 == 0 { 40 } else { 120 });
        let pool = random_pool(&mut rng, size, round % 3);
        for temperature in [1.0, 5.0] {
            for aversion in [0.0, 0.0, 1e-12, 0.4, 1.5] {
                let len = 2 + rng.below(7);
                let set = if round % 2 == 0 {
                    random_candidates(&mut rng, &pool, len)
                } else {
                    similar_candidates(&mut rng, &pool, len)
                };
                // the list holds every candidate, as the search requires
                let mut list = set.clone();
                let extra = 3 + rng.below(12);
                list.extend(random_candidates(&mut rng, &pool, extra));
                list.sort_unstable();
                list.dedup();
                for (mode, allowed) in [
                    (GuessPool::All, Allowed::Whole),
                    (GuessPool::All, Allowed::List(list)),
                    (GuessPool::Ultrahard, Allowed::Candidates),
                ] {
                    assert_matches_reference(&pool, mode, temperature, aversion, &set, &allowed);
                }
                // hard mode: the set and list a real first guess leaves
                let guess = rng.index(pool.len());
                let target = rng.index(pool.len());
                let code = pool.code(guess);
                let ordinal = pattern_ordinal(pool.code(target), code, &mut [0; 256]);
                let left: Vec<u32> = pool
                    .indices()
                    .filter(|&i| pattern_ordinal(pool.code(i), code, &mut [0; 256]) == ordinal)
                    .collect();
                if (2..=10).contains(&left.len()) {
                    let allowed = Allowed::Whole.after(GuessPool::Hard, &pool, code, ordinal);
                    assert_matches_reference(
                        &pool,
                        GuessPool::Hard,
                        temperature,
                        aversion,
                        &left,
                        &allowed,
                    );
                }
            }
        }
    }
}

#[test]
fn a_table_partition_equals_pool_partition() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for round in 0..60 {
        let size = 30 + rng.below(90);
        let pool = random_pool(&mut rng, size, round % 3);
        let len = 2 + rng.below(9);
        let set = random_candidates(&mut rng, &pool, len);
        let mut list = set.clone();
        let extra = 3 + rng.below(20);
        list.extend(random_candidates(&mut rng, &pool, extra));
        list.sort_unstable();
        list.dedup();
        for allowed in [Allowed::Whole, Allowed::List(list), Allowed::Candidates] {
            let mut table = PatternTable::default();
            table.build(&pool, &set, &allowed);
            let guesses: Vec<u32> = match &allowed {
                Allowed::Whole => pool.indices().collect(),
                Allowed::List(list) => list.clone(),
                Allowed::Candidates => set.clone(),
            };
            for guess in guesses {
                // the whole set, then a random subset of it, as a child would be
                let sub: Vec<u32> = set.iter().copied().filter(|_| rng.below(2) == 0).collect();
                for part in [&set, &sub] {
                    let (mut direct, mut tabled) = (Vec::new(), Vec::new());
                    pool.partition(part, guess, &mut direct);
                    let mut cols = Vec::new();
                    assert!(table.columns(part, &mut cols));
                    assert!(table.partition(&cols, part, guess, &mut tabled));
                    assert_eq!(direct, tabled, "guess {guess} over {part:?}");
                }
            }
        }
    }
}

#[test]
fn a_table_over_several_blocks_equals_pool_partition() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for round in 0..6 {
        let size = 200 + rng.below(50);
        let pool = random_pool(&mut rng, size, round % 3);
        let len = 65 + rng.below(100);
        let set = random_candidates(&mut rng, &pool, len);
        let mut table = PatternTable::default();
        table.build(&pool, &set, &Allowed::Whole);
        let mut cols = Vec::new();
        assert!(table.columns(&set, &mut cols));
        for guess in pool.indices() {
            let (mut direct, mut tabled) = (Vec::new(), Vec::new());
            pool.partition(&set, guess, &mut direct);
            assert!(table.partition(&cols, &set, guess, &mut tabled));
            assert_eq!(direct, tabled, "guess {guess}");
        }
    }
}

#[test]
fn floors_read_from_ranks_equal_floors_over_the_sorted_partition() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..60 {
        let size = 30 + rng.below(90);
        let pool = random_pool(&mut rng, size, round % 3);
        let len = 2 + rng.below(9);
        let set = random_candidates(&mut rng, &pool, len);
        let mut list = set.clone();
        let extra = 3 + rng.below(20);
        list.extend(random_candidates(&mut rng, &pool, extra));
        list.sort_unstable();
        list.dedup();
        for aversion in [0.0, 0.3] {
            let mut endgame = Endgame::new(shared(&pool), GuessPool::All, 1.0, aversion);
            for allowed in [
                Allowed::Whole,
                Allowed::List(list.clone()),
                Allowed::Candidates,
            ] {
                endgame.pool.fill_table(&mut endgame.table, &set, &allowed);
                endgame.table.active = true;
                // the whole set, then random subsets of it, as children are
                let mut parts = vec![set.clone()];
                for _ in 0..4 {
                    let sub: Vec<u32> = set.iter().copied().filter(|_| rng.below(2) == 0).collect();
                    if !sub.is_empty() {
                        parts.push(sub);
                    }
                }
                for part in parts {
                    let mass: f64 = part.iter().map(|&i| endgame.weight(i)).sum();
                    let guesses: Vec<u32> = match &allowed {
                        Allowed::Whole => pool.indices().collect(),
                        Allowed::List(list) => list.clone(),
                        Allowed::Candidates => part.clone(),
                    };
                    let mut expected = Vec::new();
                    let mut keyed = Vec::new();
                    for &g in &guesses {
                        pool.partition(&part, g, &mut keyed);
                        let in_set = part.binary_search(&g).is_ok();
                        if let Some(f) = endgame.floor(&keyed, g, in_set, mass) {
                            expected.push((g, !in_set, f.to_bits()));
                        }
                    }
                    let mut got: Vec<(u32, bool, u64)> = endgame
                        .floors(&part, &allowed, mass)
                        .into_iter()
                        .map(|Reverse(Ranked(f, outside, g))| (g, outside, f.to_bits()))
                        .collect();
                    got.sort_unstable();
                    expected.sort_unstable();
                    assert_eq!(expected, got, "L{aversion} {part:?} {allowed:?}");
                }
            }
        }
    }
}

#[test]
fn the_fast_paths_match_the_full_search_on_a_family_with_probes() {
    let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
    let set = indices_of(&pool, &FAMILY);
    for aversion in [0.0, 0.3, 1.0] {
        for len in 2..=FAMILY.len() {
            assert_matches_reference(
                &pool,
                GuessPool::All,
                5.0,
                aversion,
                &set[..len],
                &Allowed::Whole,
            );
        }
    }
}

/// Roots reached by real play on the English-5 cache: every 400th target is played with the
/// default strategy until at most `LIMIT` candidates remain, and each such set must satisfy the
/// same predicted-equals-simulated check as the synthetic family. Needs `data/ngrams`, so it
/// is opt-in: `cargo test -p lex-core --release -- --ignored real_corpus`.
#[test]
#[ignore = "needs the English-5 cache in data/"]
fn real_corpus_roots_match_simulated_cost() {
    use crate::candidates::Candidates;
    use crate::correctness::WordCorrectness;
    use crate::guesser::Guess;
    use crate::strategy::Strategy;

    const LIMIT: usize = 5;
    let dir = std::env::var("LEX_DATA_DIR").unwrap_or_else(|_| "../data".into());
    let word_set = lex_data::DataDir::new(dir)
        .load::<5>(lex_data::Language::English, None)
        .unwrap();
    let pool = Pool::new(&word_set);
    let strategy = Strategy::default();
    let mut endgame = Endgame::new(shared(&pool), GuessPool::All, strategy.temperature, 0.4);
    let mut checked = std::collections::HashSet::new();
    // `suggest` over the full pool is seconds at the root, so share it across targets
    let mut suggested = std::collections::HashMap::new();
    for target_index in pool.indices().step_by(400) {
        let target = pool.word(target_index);
        let mut candidates = Candidates::whole_pool(Arc::clone(&pool));
        while candidates.len() > LIMIT {
            let guess = *suggested
                .entry(candidates.indices().to_vec())
                .or_insert_with(|| candidates.suggest(&strategy, &Allowed::Whole).unwrap());
            let feedback = WordCorrectness::correct(target, guess);
            candidates = candidates.filtered(&Guess::new(guess, feedback));
        }
        let set = candidates.indices().to_vec();
        if set.len() < 2 || !checked.insert(set.clone()) {
            continue;
        }
        let started = std::time::Instant::now();
        let predicted = endgame.solve(&set, &Allowed::Whole).cost;
        eprintln!("root of {} solved in {:?}", set.len(), started.elapsed());
        let simulated = simulated_cost(&mut endgame, &pool, &set);
        eprintln!(
            "  simulated in {:?}: {predicted} vs {simulated}",
            started.elapsed()
        );
        assert!(
            (predicted - simulated).abs() < 1e-9 * predicted,
            "{predicted} vs {simulated} for {set:?}"
        );
    }
    assert!(checked.len() > 10, "only {} distinct roots", checked.len());
    eprintln!("checked {} roots", checked.len());
}
