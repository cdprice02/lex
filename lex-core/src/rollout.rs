//! One-step lookahead for mid-sized Candidate sets: play out the best-scored guesses and keep the
//! cheapest.

use std::collections::HashMap;
use std::sync::Arc;

use crate::allowed::{Allowed, AllowedIds};
use crate::candidates::Scored;
use crate::policy::BasePolicy;
use crate::pool_ops::PoolOps;
use crate::strategy::{GuessPool, Strategy};

/// The largest Candidate set a rollout is attempted on. A compiled-in constant rather than a
/// Strategy field: it bounds the worst Track's book build, not a solver preference, and it moves
/// `SOLVER_DIGEST` with the source.
pub const ROLLOUT_CAP: usize = 300;

/// How many times a greedy position's `allowed x candidates` pattern evaluations a rollout
/// position costs, amortized over the shared base-cost memo. `Decision tree` work uses it so the
/// book floor still means pattern evaluations.
pub const ROLLOUT_MULTIPLIER: u64 = 8;

/// Chooses among the top-scored guesses by the expected guesses each leaves under the base policy.
///
/// A set's base cost is a weighted sum over answers with the pool-wide prior weights, shared by
/// every game, so the memo is keyed by the sorted candidate indices and the allowed guesses alone.
#[derive(Debug)]
pub struct Rollout {
    /// The Guess pool the set indices point into.
    pool: Arc<dyn PoolOps>,
    /// The Strategy's Guess pool, which decides which guesses a position allows.
    guess_pool: GuessPool,
    /// How many top-scored guesses are played out.
    width: usize,
    /// Candidate-set size at or below which a rollout is not attempted.
    floor: usize,
    /// Answer prior weight of each pool word, unnormalized.
    weights: Vec<f64>,
    /// Each distinct allowed-guess list met so far, numbered, so a memo key is small. Belongs to
    /// `memo`.
    list_ids: AllowedIds,
    /// Expected guesses from a set under the base policy, by allowed list (`None` when the mode
    /// implies it) and then by set.
    memo: HashMap<Option<u32>, HashMap<Vec<u32>, f64>>,
    /// Pattern evaluations spent beyond the top-scored scan itself.
    #[cfg(test)]
    evaluations: std::cell::Cell<u64>,
}

impl Rollout {
    /// A rollout over `pool` playing out the best `strategy.rollout_k` guesses, active above the
    /// Endgame threshold (and above a lone pair when the Endgame is off).
    pub(crate) fn new(pool: Arc<dyn PoolOps>, strategy: &Strategy) -> Self {
        let weights = pool.weights(strategy.temperature);
        Self {
            pool,
            guess_pool: strategy.pool,
            width: strategy.rollout_k,
            floor: strategy.endgame_threshold.max(2),
            weights,
            list_ids: AllowedIds::default(),
            memo: HashMap::new(),
            #[cfg(test)]
            evaluations: std::cell::Cell::new(0),
        }
    }

    /// Whether a rollout chooses the guess for a Candidate set of `size`.
    pub(crate) const fn applies(&self, size: usize) -> bool {
        size > self.floor && size <= ROLLOUT_CAP
    }

    /// Moves the lower edge of the band to `size` candidates and above, so a run can reproduce a
    /// fixed 12-to-300 band whatever the Endgame threshold is.
    #[cfg(test)]
    pub(crate) const fn with_floor(mut self, floor: usize) -> Self {
        self.floor = floor;
        self
    }

    /// Pattern evaluations spent beyond the top-scored scans themselves.
    #[cfg(test)]
    pub(crate) const fn evaluations(&self) -> u64 {
        self.evaluations.get()
    }

    /// Adds `guesses x answers` pattern evaluations to the count.
    #[cfg(test)]
    fn count_evaluations(&self, guesses: usize, answers: usize) {
        let widen = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        self.evaluations
            .set(self.evaluations.get() + widen(guesses) * widen(answers));
    }

    /// The Answer prior weight of the pool word at `index`.
    fn weight(&self, index: u32) -> f64 {
        self.weights[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// The summed prior weight of `set`.
    fn mass(&self, set: &[u32]) -> f64 {
        set.iter().map(|&i| self.weight(i)).sum()
    }

    /// The guess among the top `width` by score whose play-out is cheapest in expected guesses.
    /// A strictly cheaper guess wins; an exact tie keeps the higher-scored one.
    pub(crate) fn best_guess(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        base: &mut BasePolicy,
    ) -> Option<u32> {
        self.choose(set, allowed, base, None)
    }

    /// As [`best_guess`](Self::best_guess), also returning each guess played out with its cost, in
    /// score order: what a rollout already computes, kept.
    pub(crate) fn best_guess_scored(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        base: &mut BasePolicy,
    ) -> Option<(u32, Vec<(Scored, f64)>)> {
        let mut played = Vec::with_capacity(self.width);
        let best = self.choose(set, allowed, base, Some(&mut played))?;
        Some((best, played))
    }

    /// The choice, pushing each played-out guess and its cost to `played` when it is given.
    fn choose(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        base: &mut BasePolicy,
        mut played: Option<&mut Vec<(Scored, f64)>>,
    ) -> Option<u32> {
        let mass = self.mass(set);
        let mut best: Option<(f64, u32)> = None;
        for scored in self.pool.top_k(set, base.strategy(), allowed, self.width) {
            let cost = self.guess_cost(set, allowed, scored.index, mass, base);
            if let Some(played) = played.as_deref_mut() {
                played.push((scored, cost));
            }
            if best.is_none_or(|(lowest, _)| cost < lowest) {
                best = Some((cost, scored.index));
            }
        }
        best.map(|(_, guess)| guess)
    }

    /// Expected guesses from `set` after playing `guess` now and the base policy afterwards, as a
    /// rollout costs a guess; infinite when it leaves the set whole.
    pub(crate) fn cost_of(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        guess: u32,
        base: &mut BasePolicy,
    ) -> f64 {
        let mass = self.mass(set);
        self.guess_cost(set, allowed, guess, mass, base)
    }

    /// Expected guesses from `set` after playing `guess` now and the base policy afterwards;
    /// infinite when the guess leaves the set whole.
    fn guess_cost(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        guess: u32,
        mass: f64,
        base: &mut BasePolicy,
    ) -> f64 {
        let mut keyed = Vec::with_capacity(set.len());
        self.pool.partition(set, guess, &mut keyed);
        #[cfg(test)]
        self.count_evaluations(1, set.len());
        let win = self.pool.win_ordinal();
        let mut cost = 1.0;
        // ascending feedback ordinal, so the floating-point sum does not depend on word order
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            if group.len() == set.len() {
                return f64::INFINITY;
            }
            let child: Vec<u32> = group.iter().map(|&(_, i)| i).collect();
            let child_allowed =
                self.pool
                    .allowed_after(allowed, self.guess_pool, guess, group[0].0);
            let share = self.mass(&child) / mass;
            let child_cost = self.base_cost(&child, &child_allowed, base);
            cost = share.mul_add(child_cost, cost);
        }
        cost
    }

    /// Expected guesses from `set` under the base policy: the Endgame within the Strategy's
    /// threshold, otherwise the best-scored guess. Memoized.
    fn base_cost(&mut self, set: &[u32], allowed: &Allowed, base: &mut BasePolicy) -> f64 {
        if set.len() <= 1 {
            return 1.0;
        }
        let slot = self.list_ids.slot(allowed);
        if let Some(&cost) = self.memo.get(&slot).and_then(|m| m.get(set)) {
            return cost;
        }
        let guess = self.base_guess(set, allowed, base);
        let cost = guess.map_or(f64::INFINITY, |guess| {
            self.guess_cost(set, allowed, guess, self.mass(set), base)
        });
        self.memo
            .entry(slot)
            .or_default()
            .insert(set.to_vec(), cost);
        cost
    }

    /// The base policy's guess for `set`.
    fn base_guess(&self, set: &[u32], allowed: &Allowed, base: &mut BasePolicy) -> Option<u32> {
        #[cfg(test)]
        if !base.uses_endgame(set.len()) {
            self.count_evaluations(allowed.guess_count(self.pool.len(), set.len()), set.len());
        }
        base.guess_index(&self.pool, set, allowed)
    }
}

#[cfg(test)]
mod tests {
    use lex_data::{Word, WordSet};

    use super::*;
    use crate::candidates::Candidates;
    use crate::correctness::{WordCorrectness, pattern_ordinal};
    use crate::pool_ops::shared;
    use crate::strategy::Pool;

    /// Words over a small alphabet, frequency-descending, so scores tie and play-outs differ.
    fn pool() -> Arc<Pool<4>> {
        let letters = ['a', 'b', 'c', 'd', 'e'];
        let mut seen = std::collections::HashSet::new();
        let pairs: Vec<(Word<4>, u64)> = (0..2000_u64)
            .map(|i| -> [char; 4] {
                let mixed = usize::try_from(i * 2_654_435_761 % 625).unwrap_or(0);
                std::array::from_fn(|p| {
                    letters[(mixed / 5_usize.pow(u32::try_from(p).unwrap())) % 5]
                })
            })
            .filter(|chars| seen.insert(*chars))
            .take(120)
            .zip(0_u64..)
            .map(|(chars, i)| (Word::from(chars), 1200 - 7 * i))
            .collect();
        Pool::new(&WordSet::from_pairs(pairs))
    }

    fn feedback(pool: &Pool<4>, answer: u32, guess: u32) -> u64 {
        pattern_ordinal(pool.code(answer), pool.code(guess), &mut [0; 256])
    }

    /// Plays `answer` out from `set` under the greedy policy and returns the guesses it takes.
    fn greedy_length(pool: &Arc<Pool<4>>, strategy: &Strategy, set: &[u32], answer: u32) -> u32 {
        let mut remaining = set.to_vec();
        let mut guesses = 0;
        loop {
            let candidates = Candidates::from_indices(Arc::clone(pool), remaining.clone());
            let guess = candidates
                .suggest_index(strategy, &Allowed::Whole)
                .expect("a non-empty set has a guess");
            guesses += 1;
            if guess == answer {
                return guesses;
            }
            let seen = feedback(pool, answer, guess);
            remaining.retain(|&i| feedback(pool, i, guess) == seen);
        }
    }

    /// The prior-weighted mean guesses over `set` if `first` is played now and greedy after.
    fn simulated_cost(pool: &Arc<Pool<4>>, strategy: &Strategy, set: &[u32], first: u32) -> f64 {
        let weights = pool.weights(strategy.temperature);
        let weight = |i: u32| weights[usize::try_from(i).unwrap()];
        let mass: f64 = set.iter().map(|&i| weight(i)).sum();
        let total: f64 = set
            .iter()
            .map(|&answer| {
                let guesses = if answer == first {
                    1
                } else {
                    let seen = feedback(pool, answer, first);
                    let rest: Vec<u32> = set
                        .iter()
                        .copied()
                        .filter(|&i| feedback(pool, i, first) == seen)
                        .collect();
                    1 + greedy_length(pool, strategy, &rest, answer)
                };
                weight(answer) * f64::from(guesses)
            })
            .sum();
        total / mass
    }

    fn strategy(rollout_k: usize) -> Strategy {
        Strategy {
            rollout_k,
            ..Strategy::default()
        }
    }

    fn choose(
        pool: &Arc<Pool<4>>,
        strategy: &Strategy,
        set: &[u32],
        allowed: &Allowed,
    ) -> Option<u32> {
        let mut rollout = Rollout::new(shared(pool), strategy);
        let mut base = BasePolicy::new(*strategy, None);
        rollout.best_guess(set, allowed, &mut base)
    }

    #[test]
    fn the_choice_is_the_top_scored_guess_with_the_cheapest_playout() {
        let pool = pool();
        let strategy = strategy(6);
        let set: Vec<u32> = (0..120).step_by(4).collect();
        let candidates = Candidates::from_indices(Arc::clone(&pool), set.clone());
        let top = candidates.suggest_top_k(&strategy, &Allowed::Whole, 6);
        // an independent play-out of each shortlisted guess; the first strictly cheapest wins
        let costs: Vec<f64> = top
            .iter()
            .map(|s| simulated_cost(&pool, &strategy, &set, s.index))
            .collect();
        let mut winner = 0;
        for (i, cost) in costs.iter().enumerate() {
            if *cost < costs[winner] - 1e-9 {
                winner = i;
            }
        }
        assert!(
            costs.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-6),
            "the shortlist must differ in cost for the test to mean anything: {costs:?}"
        );
        assert_eq!(
            choose(&pool, &strategy, &set, &Allowed::Whole),
            Some(top[winner].index)
        );
    }

    #[test]
    fn a_shortlist_of_one_is_the_greedy_guess() {
        let pool = pool();
        let strategy = strategy(1);
        let set: Vec<u32> = (0..120).step_by(3).collect();
        let greedy = Candidates::from_indices(Arc::clone(&pool), set.clone())
            .suggest_index(&strategy, &Allowed::Whole);
        assert_eq!(choose(&pool, &strategy, &set, &Allowed::Whole), greedy);
    }

    #[test]
    fn the_band_runs_above_the_endgame_threshold_up_to_the_cap() {
        let pool = pool();
        let rollout = |threshold| {
            Rollout::new(
                shared(&pool),
                &Strategy {
                    endgame_threshold: threshold,
                    ..strategy(10)
                },
            )
        };
        let off = rollout(0);
        assert!(!off.applies(2) && off.applies(3) && off.applies(ROLLOUT_CAP));
        assert!(!off.applies(ROLLOUT_CAP + 1));
        let with_endgame = rollout(12);
        assert!(!with_endgame.applies(12) && with_endgame.applies(13));
    }

    #[test]
    fn the_memo_never_reuses_a_cost_across_different_allowed_guesses() {
        let pool = pool();
        let strategy = strategy(4);
        let set: Vec<u32> = (0..120).step_by(5).collect();
        let restricted = Allowed::List(set.clone());
        let mut rollout = Rollout::new(shared(&pool), &strategy);
        let mut base = BasePolicy::new(strategy, None);
        let limited = rollout.base_cost(&set, &restricted, &mut base);
        let free = rollout.base_cost(&set, &Allowed::Whole, &mut base);
        assert!(free < limited, "{free} !< {limited}");
        assert_eq!(
            rollout.base_cost(&set, &restricted, &mut base).to_bits(),
            limited.to_bits()
        );
        let fresh = Rollout::new(shared(&pool), &strategy).base_cost(&set, &restricted, &mut base);
        assert_eq!(limited.to_bits(), fresh.to_bits());
    }

    /// The spike's English-5 "rollout K=10, 12 to 300" row, on the pre-ingest Corpus: the same
    /// prior-weighted mean to four decimals, and with `SPIKE_TARGETS` naming the spike's
    /// per-target file (`word<TAB>guesses` lines), every target's guess count. Also prints the
    /// measured rollout multiplier. Needs the English-5 cache, so it is opt-in:
    /// `LEX_DATA_DIR=... cargo test -p lex-core --release --lib -- --ignored real_corpus_rollout --nocapture`.
    #[test]
    #[ignore = "needs the English-5 cache in data/ and about five minutes"]
    fn real_corpus_rollout_reproduces_the_spike_band() {
        use crate::guesser::Guess;
        use crate::tree::DecisionTree;

        let dir = std::env::var("LEX_DATA_DIR").unwrap_or_else(|_| "../data".into());
        let word_set = lex_data::DataDir::new(dir)
            .load::<5>(lex_data::Language::English, None)
            .unwrap();
        let pool = Pool::new(&word_set);
        let answers: Vec<Word<5>> = pool.indices().map(|i| pool.word(i)).collect();
        let strategy = Strategy {
            rollout_k: 10,
            ..Strategy::default()
        };
        let mut tree =
            DecisionTree::new(Candidates::new(Arc::clone(&pool), &answers), strategy, None);
        // the band is 12 to 300 candidates whatever the Endgame threshold, as in the spike
        tree.set_rollout_floor(11);
        let all: Vec<u32> = pool.indices().collect();
        let prior = pool.prior(&all, crate::strategy::CANONICAL_TEMPERATURE);
        let (mut uniform, mut weighted) = (0.0, 0.0);
        let mut guesses_by_word = HashMap::new();
        for (&target, &p) in answers.iter().zip(&prior) {
            let mut node = DecisionTree::<5>::ROOT;
            let mut guesses = 0_u32;
            loop {
                let guess = tree.suggest(node).unwrap();
                guesses += 1;
                let feedback = WordCorrectness::correct(target, guess);
                if feedback.is_correct() {
                    break;
                }
                node = tree.child(node, &Guess::new(guess, feedback));
            }
            uniform += f64::from(guesses);
            weighted = p.mul_add(f64::from(guesses), weighted);
            guesses_by_word.insert(target.to_string(), guesses);
        }
        uniform /= f64::from(u32::try_from(answers.len()).unwrap());
        let (spent, scans) = tree.rollout_cost();
        eprintln!("uniform {uniform:.4} weighted {weighted:.4}");
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "a printed ratio of counts far below 2^53"
        )]
        let multiplier = spent as f64 / scans as f64;
        eprintln!(
            "rollout evaluations {spent} over {scans} scan evaluations: multiplier {multiplier:.2}"
        );
        assert_eq!(format!("{weighted:.4}"), "4.1569");
        if let Ok(path) = std::env::var("SPIKE_TARGETS") {
            let spike = std::fs::read_to_string(path).unwrap();
            let mut differ = 0;
            for line in spike.lines() {
                let (word, guesses) = line.split_once('\t').unwrap();
                if guesses_by_word[word] != guesses.parse::<u32>().unwrap() {
                    differ += 1;
                }
            }
            eprintln!(
                "{differ} of {} targets differ from the spike",
                guesses_by_word.len()
            );
            assert_eq!(differ, 0);
        }
    }
}

#[cfg(feature = "bench")]
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "bench functions are named for the divan report, not documented; clippy skips them in the test harness build"
    )
)]
mod benches {
    use divan::Bencher;
    use std::hint::black_box;

    use std::sync::Arc;

    use super::{Allowed, BasePolicy, Rollout, Strategy};
    use crate::candidates::Candidates;
    use crate::candidates::synthetic_pool;
    use crate::endgame::Endgame;
    use crate::pool_ops::shared;

    /// Candidate-set sizes the rollout multiplier is read at: the edge of the band, a mid-sized
    /// set, and the compiled-in cap.
    const SIZES: [usize; 3] = [12, 60, 300];

    /// The first `size` words of a 3000-word pool as a Candidate set, so the guesses are scored
    /// against a pool far larger than the set, as deep game positions are.
    fn fixture(size: usize) -> Candidates<5> {
        let pool = synthetic_pool::<5>(3000);
        Candidates::from_indices(pool, (0..u32::try_from(size).unwrap_or(0)).collect())
    }

    /// The greedy scan a rollout position is measured against.
    #[divan::bench(args = SIZES)]
    fn greedy_suggest(b: Bencher, size: usize) {
        let candidates = fixture(size);
        let strategy = Strategy::default();
        b.bench(|| black_box(candidates.suggest_index(&strategy, &Allowed::Whole)));
    }

    /// One rollout from a cold memo, K=10, with greedy as the base policy: its ratio to
    /// `greedy_suggest` is the multiplier a position pays before the memo is shared across games.
    #[divan::bench(args = SIZES)]
    fn rollout_cold(b: Bencher, size: usize) {
        let candidates = fixture(size);
        let strategy = Strategy {
            endgame_threshold: 0,
            rollout_k: 10,
            ..Strategy::default()
        };
        b.with_inputs(|| Rollout::new(shared(&candidates.pool()), &strategy))
            .bench_local_refs(|rollout| {
                let mut base = BasePolicy::new(strategy, None);
                black_box(rollout.best_guess(candidates.indices(), &Allowed::Whole, &mut base))
            });
    }

    /// [`rollout_cold`] with the Endgame inside its threshold as the base policy, the tree's
    /// default: the multiplier a position pays when children within the threshold are searched.
    #[divan::bench(args = SIZES)]
    fn rollout_cold_endgame(b: Bencher, size: usize) {
        let candidates = fixture(size);
        let strategy = Strategy {
            rollout_k: 10,
            ..Strategy::default()
        };
        b.with_inputs(|| {
            let pool = shared(&candidates.pool());
            let endgame = Endgame::new(
                Arc::clone(&pool),
                strategy.pool,
                strategy.temperature,
                strategy.endgame_aversion,
            );
            (
                Rollout::new(pool, &strategy),
                BasePolicy::new(strategy, Some(endgame)),
            )
        })
        .bench_local_refs(|(rollout, base)| {
            black_box(rollout.best_guess(candidates.indices(), &Allowed::Whole, base))
        });
    }
}
