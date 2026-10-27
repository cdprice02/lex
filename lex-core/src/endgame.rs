use std::collections::HashMap;
use std::sync::Arc;

use lex_data::Word;

use crate::correctness::{LetterCounts, WordCorrectness, pattern_ordinal};
use crate::strategy::{GuessPool, Pool};

/// The best guess for a candidate set and what it costs.
#[derive(Debug, Clone, Copy)]
struct Solved {
    /// The sum over answers in the set of `weight * f(guesses to finish)`, where `f` is the
    /// identity, or `a^guesses` with risk aversion.
    cost: f64,
    guess: u32,
}

/// Exact search over the game tree for small candidate sets, minimizing expected guesses under the
/// answer prior, or with risk aversion the expected `a^guesses`. The exponential keeps the cost of
/// a set independent of how many guesses were already played, which is what lets the memo key be
/// the set alone; a penalty for passing a fixed guess count would not.
///
/// Costs are kept as weighted sums rather than probabilities, with weights taken once from the
/// whole pool, so a set's cost does not depend on which game position it was reached from and the
/// memo (keyed by the sorted candidate indices) is shared by every Endgame root and every game.
/// The search is exhaustive, with no bound passed down, so memoized costs are always exact.
#[derive(Debug)]
pub(crate) struct Endgame<const N: usize> {
    pool: Arc<Pool<N>>,
    guess_pool: GuessPool,
    /// `a` in the `a^guesses` objective; `None` is plain expected guesses (the `a -> 1` limit).
    growth: Option<f64>,
    weights: Vec<f64>,
    memo: HashMap<Vec<u32>, Solved>,
}

impl<const N: usize> Endgame<N> {
    pub(crate) fn new(
        pool: Arc<Pool<N>>,
        guess_pool: GuessPool,
        temperature: f64,
        aversion: f64,
    ) -> Self {
        let weights = pool.weights(temperature);
        Self {
            pool,
            guess_pool,
            growth: (aversion > 0.0).then(|| aversion.exp()),
            weights,
            memo: HashMap::new(),
        }
    }

    /// The guess minimizing expected guesses for the answers in `set` (ascending pool indices).
    pub(crate) fn best_guess(&mut self, set: &[u32]) -> Option<Word<N>> {
        if set.is_empty() {
            return None;
        }
        let guess = self.solve(set).guess;
        Some(self.pool.word(guess))
    }

    /// Expected number of guesses to finish from `set` under the answer prior renormalized to it.
    #[cfg(test)]
    pub(crate) fn expected_guesses(&mut self, set: &[u32]) -> f64 {
        let mass: f64 = set.iter().map(|&i| self.weights[i as usize]).sum();
        self.solve(set).cost / mass
    }

    fn solve(&mut self, set: &[u32]) -> Solved {
        if let Some(&solved) = self.memo.get(set) {
            return solved;
        }
        let solved = match *set {
            // a lone candidate is the answer: one guess
            [only] => Solved {
                cost: self.weights[only as usize] * self.growth.unwrap_or(1.0),
                guess: only,
            },
            _ => self.search(set),
        };
        self.memo.insert(set.to_vec(), solved);
        solved
    }

    /// Finds the cheapest guess, trying them in order of a cheap lower bound on their cost and
    /// stopping once the bound exceeds the best found. Most of the pool is rejected by the bound
    /// without recursing into the groups it would leave. Exact ties go to a candidate, then to the
    /// more frequent word, as in the entropy solver.
    fn search(&mut self, set: &[u32]) -> Solved {
        let mass: f64 = set.iter().map(|&i| self.weights[i as usize]).sum();
        let mut keyed = Vec::with_capacity(set.len());
        let mut order: Vec<(f64, bool, u32)> = Vec::new();
        let mut consider = |this: &Self, g: u32, in_set: bool| {
            this.partition(set, g, &mut keyed);
            if let Some(floor) = this.floor(&keyed, g, in_set, mass) {
                order.push((floor, !in_set, g));
            }
        };
        for &g in set {
            consider(self, g, true);
        }
        if self.guess_pool == GuessPool::All {
            let mut next = 0;
            for g in 0..self.pool.len() as u32 {
                if set.get(next) == Some(&g) {
                    next += 1;
                } else {
                    consider(self, g, false);
                }
            }
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

        let mut best: Option<Solved> = None;
        for (floor, outside, g) in order {
            // the slack keeps a bound that rounds a hair high from hiding a true tie or win
            if best.is_some_and(|b| floor > b.cost * (1.0 + 1e-9)) {
                break;
            }
            self.try_guess(set, g, !outside, mass, &mut best);
        }
        best.expect("a non-empty set has a candidate guess")
    }

    /// Fills `out` with `(feedback ordinal, answer)` for every answer in `set` against `guess`,
    /// grouped by ordinal and ascending by index within a group.
    fn partition(&self, set: &[u32], guess: u32, out: &mut Vec<(u64, u32)>) {
        let code = self.pool.code(guess);
        let mut counts: LetterCounts = [0; 256];
        out.clear();
        out.extend(
            set.iter()
                .map(|&i| (pattern_ordinal(self.pool.code(i), code, &mut counts), i)),
        );
        // stable sort keeps each group ascending by index
        out.sort_by_key(|&(ordinal, _)| ordinal);
    }

    /// A lower bound on the cost of `guess`, or `None` if it should not be tried. A non-candidate
    /// that cannot tell the answers apart only delays the game, and skipping it is what guarantees
    /// the recursion shrinks the set.
    ///
    /// A group of unsolved answers needs at least one guess for the one it names and two for every
    /// other (one more when only plain expectation counts), which is exact for a lone answer.
    fn floor(&self, keyed: &[(u64, u32)], guess: u32, in_set: bool, mass: f64) -> Option<f64> {
        if !in_set && keyed.first().map(|k| k.0) == keyed.last().map(|k| k.0) {
            return None;
        }
        let win = WordCorrectness::<N>::COUNT - 1;
        let mut sum = self.initial_cost(guess, in_set, mass);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            let weights = group.iter().map(|&(_, i)| self.weights[i as usize]);
            let (group_mass, top) = weights.fold((0.0, 0.0_f64), |(m, t), w| (m + w, t.max(w)));
            sum += match self.growth {
                None => 2.0 * group_mass - top,
                Some(a) => a * top + a * a * (group_mass - top),
            };
        }
        Some(sum * self.growth.unwrap_or(1.0))
    }

    /// What the guess itself costs before any unsolved group is added. Plain: every answer pays
    /// this turn. Averse: only a win pays `a^1`, every other answer pays `a` times its group.
    fn initial_cost(&self, guess: u32, in_set: bool, mass: f64) -> f64 {
        match self.growth {
            None => mass,
            Some(_) if in_set => self.weights[guess as usize],
            Some(_) => 0.0,
        }
    }

    /// Scores `guess` against `set` and keeps it in `best` if cheaper, or equal and ranked ahead.
    fn try_guess(
        &mut self,
        set: &[u32],
        guess: u32,
        in_set: bool,
        mass: f64,
        best: &mut Option<Solved>,
    ) {
        let mut keyed = Vec::with_capacity(set.len());
        self.partition(set, guess, &mut keyed);

        let win = WordCorrectness::<N>::COUNT - 1;
        let growth = self.growth.unwrap_or(1.0);
        let mut cost = self.initial_cost(guess, in_set, mass);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            let child: Vec<u32> = group.iter().map(|&(_, i)| i).collect();
            cost += self.solve(&child).cost;
            // costs only grow from here, so this guess can no longer be as cheap
            if best.is_some_and(|b| cost * growth > b.cost) {
                return;
            }
        }
        let cost = cost * growth;
        let ahead = |b: Solved| {
            // candidates rank ahead of non-candidates, then pool (frequency) order
            let rank = |g: u32| (set.binary_search(&g).is_err(), g);
            cost < b.cost || (cost == b.cost && rank(guess) < rank(b.guess))
        };
        if best.is_none_or(ahead) {
            *best = Some(Solved { cost, guess });
        }
    }
}

#[cfg(test)]
mod tests {
    use lex_data::WordSet;

    use super::*;

    const FAMILY: [&str; 7] = [
        "batch", "catch", "hatch", "latch", "match", "patch", "watch",
    ];

    fn pool_of(words: &[&str]) -> Arc<Pool<5>> {
        let pairs = words
            .iter()
            .enumerate()
            .map(|(i, s)| (Word::<5>::try_from(*s).unwrap(), 1000 - i as u64))
            .collect();
        Pool::new(&WordSet::from_pairs(pairs))
    }

    fn ordinal_of(pool: &Pool<5>, word: Word<5>, guess: Word<5>) -> u64 {
        pattern_ordinal(&pool.encode(&word), &pool.encode(&guess), &mut [0; 256])
    }

    fn indices_of(pool: &Pool<5>, words: &[&str]) -> Vec<u32> {
        let mut v: Vec<u32> = words
            .iter()
            .map(|w| {
                let w = Word::<5>::try_from(*w).unwrap();
                (0..pool.len() as u32).find(|&i| pool.word(i) == w).unwrap()
            })
            .collect();
        v.sort_unstable();
        v
    }

    /// Plays every answer in the set with the endgame's own guesses and returns the weighted sum
    /// of the objective over the games: the number the search must have predicted as its cost.
    fn simulated_cost(endgame: &mut Endgame<5>, set: &[u32]) -> f64 {
        let mut total = 0.0;
        for &answer in set {
            let target = endgame.pool.word(answer);
            let mut remaining = set.to_vec();
            let mut guesses = 0;
            loop {
                let guess = endgame.best_guess(&remaining).unwrap();
                guesses += 1;
                if guess == target {
                    break;
                }
                let feedback = ordinal_of(&endgame.pool, target, guess);
                remaining.retain(|&i| {
                    ordinal_of(&endgame.pool, endgame.pool.word(i), guess) == feedback
                });
            }
            let objective = endgame.growth.map_or(guesses as f64, |a| a.powi(guesses));
            total += endgame.weights[answer as usize] * objective;
        }
        total
    }

    #[test]
    fn predicted_cost_matches_simulated_cost() {
        let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
        let set = indices_of(&pool, &FAMILY);
        for aversion in [0.0, 0.3, 1.0] {
            let mut endgame = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, aversion);
            let predicted = endgame.solve(&set).cost;
            let simulated = simulated_cost(&mut endgame, &set);
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
            let mut endgame = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, aversion);
            set.iter()
                .map(|&answer| {
                    let target = endgame.pool.word(answer);
                    let mut remaining = set.clone();
                    let mut guesses = 0;
                    loop {
                        let guess = endgame.best_guess(&remaining).unwrap();
                        guesses += 1;
                        if guess == target {
                            return guesses;
                        }
                        let feedback = ordinal_of(&endgame.pool, target, guess);
                        remaining.retain(|&i| {
                            ordinal_of(&endgame.pool, endgame.pool.word(i), guess) == feedback
                        });
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
        let all = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, 0.0).expected_guesses(&set);
        let hard =
            Endgame::new(Arc::clone(&pool), GuessPool::Candidates, 5.0, 0.0).expected_guesses(&set);
        assert!(all < hard, "{all} !< {hard}");
    }

    #[test]
    fn a_lone_candidate_is_one_guess() {
        let pool = pool_of(&FAMILY);
        let mut endgame = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, 0.0);
        assert_eq!(endgame.expected_guesses(&[3]), 1.0);
        assert_eq!(endgame.best_guess(&[3]), Some(pool.word(3)));
        assert_eq!(endgame.best_guess(&[]), None);
    }

    #[test]
    fn two_candidates_cost_one_and_a_half_guesses_when_equally_likely() {
        let pool = Pool::new(&WordSet::from_pairs(vec![
            (Word::<5>::try_from("crane").unwrap(), 10),
            (Word::<5>::try_from("crate").unwrap(), 10),
        ]));
        let mut endgame = Endgame::new(pool, GuessPool::All, 5.0, 0.0);
        assert!((endgame.expected_guesses(&[0, 1]) - 1.5).abs() < 1e-12);
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
        let mut endgame =
            Endgame::new(Arc::clone(&pool), GuessPool::All, strategy.temperature, 0.4);
        let mut checked = std::collections::HashSet::new();
        // `suggest` over the full pool is seconds at the root, so share it across targets
        let mut suggested = std::collections::HashMap::new();
        for target_index in (0..pool.len() as u32).step_by(400) {
            let target = pool.word(target_index);
            let mut candidates = Candidates::whole_pool(Arc::clone(&pool));
            while candidates.len() > LIMIT {
                let guess = *suggested
                    .entry(candidates.indices().to_vec())
                    .or_insert_with(|| candidates.suggest(&strategy).unwrap());
                let feedback = WordCorrectness::correct(target, guess);
                candidates = candidates.filtered(&Guess::new(guess, feedback));
            }
            let set = candidates.indices().to_vec();
            if set.len() < 2 || !checked.insert(set.clone()) {
                continue;
            }
            let started = std::time::Instant::now();
            let predicted = endgame.solve(&set).cost;
            eprintln!("root of {} solved in {:?}", set.len(), started.elapsed());
            let simulated = simulated_cost(&mut endgame, &set);
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
}
