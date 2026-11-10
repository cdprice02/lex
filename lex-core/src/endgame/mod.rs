//! Exact search for Candidate sets at or below the Endgame threshold.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::sync::Arc;

use crate::allowed::{Allowed, AllowedIds};
use crate::pool_ops::PoolOps;
use crate::strategy::GuessPool;

mod floors;
mod table;

use floors::{Ranked, SLACK};
use table::MAX_TABLE_COLUMNS;
pub use table::PatternTable;

/// The best guess for a candidate set and what it costs.
#[derive(Debug, Clone, Copy)]
struct Solved {
    /// The sum over answers in the set of `weight * f(guesses to finish)`, where `f` is the
    /// identity, or `a^guesses` with risk aversion.
    cost: f64,
    /// Index into the Guess pool of the best guess.
    guess: u32,
}

/// Exact search over the game tree for small candidate sets, minimizing expected guesses under the
/// answer prior, or with risk aversion the expected `a^guesses`. The exponential keeps the cost of
/// a set independent of how many guesses were already played, which is what lets the memo key be
/// the set alone; a penalty for passing a fixed guess count would not.
///
/// Costs are kept as weighted sums rather than probabilities, with weights taken once from the
/// whole pool, so a set's cost does not depend on which game position it was reached from and the
/// memo is shared by every Endgame root and every game. The memo is keyed by the sorted candidate
/// indices and the allowed guesses, because the same set costs more when fewer guesses are
/// permitted; in normal mode and on an ultrahard Corpus run the allowed set is implied, so the
/// key is the candidates alone. The search is exhaustive, with no bound passed down, so memoized
/// costs are always exact.
#[derive(Debug)]
pub struct Endgame {
    /// The Guess pool the set indices point into.
    pool: Arc<dyn PoolOps>,
    /// The Strategy's Guess pool, which decides which guesses a position allows.
    guess_pool: GuessPool,
    /// `a` in the `a^guesses` objective; `None` is plain expected guesses (the `a -> 1` limit).
    growth: Option<f64>,
    /// Answer prior weight of each pool word, unnormalized.
    weights: Vec<f64>,
    /// Each distinct allowed-guess list met so far, numbered, so a memo key is small. Belongs to
    /// `memo`.
    list_ids: AllowedIds,
    /// Solved sets by allowed-guess list (`None` when the mode implies it) and then by set.
    memo: HashMap<Option<u32>, HashMap<Vec<u32>, Solved>>,
    /// The patterns of the top-level call in progress, reused between calls.
    table: PatternTable,
    /// Scans every allowed guess at every node, as the search did before the candidate-first cut:
    /// the reference the equivalence test holds the fast paths to.
    #[cfg(test)]
    reference: bool,
}

impl Endgame {
    /// An empty search over `pool`, weighting answers by the Answer prior at `temperature` and scoring risk by `aversion`.
    pub(crate) fn new(
        pool: Arc<dyn PoolOps>,
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
            list_ids: AllowedIds::default(),
            memo: HashMap::new(),
            table: PatternTable::default(),
            #[cfg(test)]
            reference: false,
        }
    }

    /// The table columns of `set`, computed once per call and passed to [`Self::partition`]:
    /// `None` when no table is active, or when a member lies outside it (a broken subset
    /// invariant, caught by a debug assertion), and then every partition is computed directly.
    fn columns(&self, set: &[u32]) -> Option<Vec<usize>> {
        if !self.table.active {
            return None;
        }
        let mut cols = Vec::with_capacity(set.len());
        if self.table.columns(set, &mut cols) {
            return Some(cols);
        }
        debug_assert!(false, "set {set:?} is outside the table");
        None
    }

    /// Fills `out` with `set` against `guess` as [`Pool::partition`] does, from the table when
    /// `cols` (from [`Self::columns`]) is given. A guess outside the table breaks the subset
    /// invariant and is computed directly in release builds.
    fn partition(
        &self,
        cols: Option<&[usize]>,
        set: &[u32],
        guess: u32,
        out: &mut Vec<(u64, u32)>,
    ) {
        if let Some(cols) = cols {
            if self.table.partition(cols, set, guess, out) {
                return;
            }
            debug_assert!(false, "guess {guess} is outside the table");
        }
        self.pool.partition(set, guess, out);
    }

    /// The Answer prior weight of the pool word at `index`.
    fn weight(&self, index: u32) -> f64 {
        self.weights[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// The guess minimizing expected guesses for the answers in `set` (ascending pool indices).
    ///
    /// The guess as a pool index.
    pub(crate) fn best_index(&mut self, set: &[u32], allowed: &Allowed) -> Option<u32> {
        if set.is_empty() {
            return None;
        }
        Some(self.solve(set, allowed).guess)
    }

    /// The summed prior weight of `set`.
    fn mass(&self, set: &[u32]) -> f64 {
        set.iter().map(|&i| self.weight(i)).sum()
    }

    /// A summed cost over a set of weight `mass` as a Guess cost: plain, the expected guesses;
    /// averse, the certainty equivalent ln(E[a^g]) / ln(a).
    fn guess_cost(&self, total: f64, mass: f64) -> f64 {
        self.growth
            .map_or_else(|| total / mass, |a| (total / mass).log(a))
    }

    /// As [`best_index`](Self::best_index), with the Guess cost of the guess.
    pub(crate) fn best_with_cost(&mut self, set: &[u32], allowed: &Allowed) -> Option<(u32, f64)> {
        if set.is_empty() {
            return None;
        }
        let solved = self.solve(set, allowed);
        Some((solved.guess, self.guess_cost(solved.cost, self.mass(set))))
    }

    /// The Guess cost of playing `guess`, allowed or not, now and the search after: one `try_guess`
    /// worth of work, the groups it leaves solved through the memo. Infinite when a guess that
    /// cannot be the answer leaves the set whole, which the search never recurses into.
    ///
    /// The pattern table is built for `set` and `allowed` so the children read it, as under
    /// [`solve`](Self::solve); the guess itself is partitioned directly, as it may not be a row.
    pub(crate) fn cost_of(&mut self, set: &[u32], allowed: &Allowed, guess: u32) -> f64 {
        let mass = self.mass(set);
        let win = self.pool.win_ordinal();
        let mut keyed = Vec::with_capacity(set.len());
        self.pool.partition(set, guess, &mut keyed);
        if keyed.iter().all(|k| k.0 == keyed[0].0) && keyed[0].0 != win {
            return f64::INFINITY;
        }
        self.ensure_table(set, allowed);
        let growth = self.growth.unwrap_or(1.0);
        let in_set = set.binary_search(&guess).is_ok();
        let mut cost = self.initial_cost(guess, in_set, mass);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            let child: Vec<u32> = group.iter().map(|&(_, i)| i).collect();
            let child_allowed =
                self.pool
                    .allowed_after(allowed, self.guess_pool, guess, group[0].0);
            cost += self.solve_within(&child, &child_allowed).cost;
        }
        self.table.active = false;
        self.guess_cost(cost * growth, mass)
    }

    /// Expected number of guesses to finish from `set` under the answer prior renormalized to it.
    #[cfg(test)]
    pub(crate) fn expected_guesses(&mut self, set: &[u32], allowed: &Allowed) -> f64 {
        let mass: f64 = set.iter().map(|&i| self.weight(i)).sum();
        self.solve(set, allowed).cost / mass
    }

    /// The best guess and cost for `set`, memoized: the top-level entry. The recursion goes
    /// through [`Self::solve_within`], so no entry nests in another and the pattern table, built
    /// by the first scan (see [`Self::scan`]), is dropped when the entry returns.
    fn solve(&mut self, set: &[u32], allowed: &Allowed) -> Solved {
        let slot = self.list_ids.slot(allowed);
        if let Some(&solved) = self.memo.get(&slot).and_then(|m| m.get(set)) {
            return solved;
        }
        let solved = self.solve_within(set, allowed);
        self.table.active = false;
        solved
    }

    /// The best guess and cost for `set`, memoized, reading patterns from the table of the
    /// top-level call.
    fn solve_within(&mut self, set: &[u32], allowed: &Allowed) -> Solved {
        let slot = self.list_ids.slot(allowed);
        if let Some(&solved) = self.memo.get(&slot).and_then(|m| m.get(set)) {
            return solved;
        }
        let solved = match *set {
            // a lone candidate is the answer: one guess
            [only] => Solved {
                cost: self.weight(only) * self.growth.unwrap_or(1.0),
                guess: only,
            },
            _ => self.search(set, allowed),
        };
        self.memo
            .entry(slot)
            .or_default()
            .insert(set.to_vec(), solved);
        solved
    }

    /// Finds the cheapest guess for a set of two or more. Two candidates, and three in plain
    /// mode, are costed in closed form, and the pool is scanned only when an outside guess could
    /// still tie or win (see [`Self::outside_cannot_win`]); every other set scans.
    fn search(&mut self, set: &[u32], allowed: &Allowed) -> Solved {
        let mass: f64 = set.iter().map(|&i| self.weight(i)).sum();
        #[cfg(test)]
        if self.reference {
            return self.scan_all(set, allowed, mass);
        }
        let closed = match (self.growth, set.len()) {
            (None, 2) => Some(self.pair(set, mass)),
            (None, 3) => Some(self.triple(set, mass)),
            _ => None,
        };
        if let Some(best) = closed {
            // a set's candidates are always allowed guesses: `Whole` holds every pool word,
            // `Candidates` is the set itself, and a `List` is built to contain them
            let outside = match allowed {
                Allowed::Candidates => false,
                Allowed::Whole => self.pool.len() > set.len(),
                Allowed::List(list) => list.len() > set.len(),
            };
            if !outside || Self::outside_cannot_win(best.cost, mass) {
                return best;
            }
        }
        self.scan(set, allowed, mass)
    }

    /// Builds the pattern table for this top-level call before its first scan. A closed form that
    /// returns never gets here, so the table is paid for only when the pool is scanned. The first
    /// scan of a call is always its top-level set, because the recursion starts inside a scan:
    /// every later scan finds the table active and reads it. Sets over the rank limit scan with
    /// `Pool::partition`.
    fn ensure_table(&mut self, set: &[u32], allowed: &Allowed) {
        if !self.table.active && set.len() <= MAX_TABLE_COLUMNS {
            self.pool.fill_table(&mut self.table, set, allowed);
        }
    }

    /// Finds the cheapest guess, trying them in order of a cheap lower bound on their cost and
    /// stopping once the bound exceeds the best found. Most of the pool is rejected by the bound
    /// without recursing into the groups it would leave, so the order is drawn from a heap: the
    /// few guesses tried cost a pop each, where sorting the whole pool would cost far more. The
    /// key is unique per guess, so the order is the one a full sort gives. Exact ties go to a
    /// candidate, then to the more frequent word, as in the entropy solver.
    #[expect(
        clippy::expect_used,
        reason = "every member of the set is tried as a guess and none is skipped, so a non-empty set always yields one"
    )]
    fn scan(&mut self, set: &[u32], allowed: &Allowed, mass: f64) -> Solved {
        self.ensure_table(set, allowed);
        let mut heap = BinaryHeap::from(self.floors(set, allowed, mass));
        let mut best: Option<Solved> = None;
        while let Some(Reverse(Ranked(floor, outside, g))) = heap.pop() {
            // the slack keeps a bound that rounds a hair high from hiding a true tie or win
            if best.is_some_and(|b| floor > b.cost * (1.0 + SLACK)) {
                break;
            }
            self.try_guess(set, allowed, g, !outside, mass, &mut best);
        }
        best.expect("a non-empty set has a candidate guess")
    }

    /// The best guess for two candidates, which is always a candidate: guessing one finishes it
    /// and leaves the other a lone group, and an outside guess costs at least `2 * mass`. The
    /// sums repeat `try_guess` operation for operation so the costs match bit for bit; the
    /// earlier candidate keeps an exact tie.
    fn pair(&self, set: &[u32], mass: f64) -> Solved {
        let cost_of = |guess: u32, other: u32| {
            let lone = self.weight(other) * self.growth.unwrap_or(1.0);
            (self.initial_cost(guess, true, mass) + lone) * self.growth.unwrap_or(1.0)
        };
        let (first, second) = (set[0], set[1]);
        let (a, b) = (cost_of(first, second), cost_of(second, first));
        if b.total_cmp(&a) == Ordering::Less {
            Solved {
                cost: b,
                guess: second,
            }
        } else {
            Solved {
                cost: a,
                guess: first,
            }
        }
    }

    /// The best candidate for three candidates in plain mode, with each group of unsolved answers
    /// costed in closed form: a lone answer is its weight, a pair is [`Self::pair`]. This is
    /// exactly the candidate pass of [`Self::scan`], because a pair's optimum is a candidate and
    /// so needs no memo. The pass is not the whole answer when `outside_cannot_win` fails: an
    /// outside guess that separates all three costs `2 * mass`, which only an (almost) exact tie
    /// among the weights leaves a candidate unable to beat.
    fn triple(&self, set: &[u32], mass: f64) -> Solved {
        let win = self.pool.win_ordinal();
        let mut keyed = Vec::with_capacity(set.len());
        let mut best = Solved {
            cost: f64::INFINITY,
            guess: set[0],
        };
        let cols = self.columns(set);
        for &guess in set {
            self.partition(cols.as_deref(), set, guess, &mut keyed);
            let mut cost = mass;
            for group in keyed.chunk_by(|a, b| a.0 == b.0) {
                if group[0].0 == win {
                    continue;
                }
                cost += if group.len() == 1 {
                    self.weight(group[0].1)
                } else {
                    let pair = [group[0].1, group[1].1];
                    self.pair(&pair, pair.iter().map(|&i| self.weight(i)).sum())
                        .cost
                };
            }
            if cost.total_cmp(&best.cost) == Ordering::Less {
                best = Solved { cost, guess };
            }
        }
        best
    }

    /// The single pass with the whole order sorted up front, which the heap-ordered `scan` must
    /// match bit for bit.
    #[cfg(test)]
    fn scan_all(&mut self, set: &[u32], allowed: &Allowed, mass: f64) -> Solved {
        let mut order: Vec<Ranked> = self
            .floors(set, allowed, mass)
            .into_iter()
            .map(|Reverse(r)| r)
            .collect();
        order.sort();
        let mut best: Option<Solved> = None;
        for Ranked(floor, outside, g) in order {
            if best.is_some_and(|b| floor > b.cost * (1.0 + SLACK)) {
                break;
            }
            self.try_guess(set, allowed, g, !outside, mass, &mut best);
        }
        best.expect("a non-empty set has a candidate guess")
    }

    /// What the guess itself costs before any unsolved group is added. Plain: every answer pays
    /// this turn. Averse: only a win pays `a^1`, every other answer pays `a` times its group.
    fn initial_cost(&self, guess: u32, in_set: bool, mass: f64) -> f64 {
        match self.growth {
            None => mass,
            Some(_) if in_set => self.weight(guess),
            Some(_) => 0.0,
        }
    }

    /// Scores `guess` against `set` and keeps it in `best` if cheaper, or equal and ranked ahead.
    fn try_guess(
        &mut self,
        set: &[u32],
        allowed: &Allowed,
        guess: u32,
        in_set: bool,
        mass: f64,
        best: &mut Option<Solved>,
    ) {
        let mut keyed = Vec::with_capacity(set.len());
        let cols = self.columns(set);
        self.partition(cols.as_deref(), set, guess, &mut keyed);

        let win = self.pool.win_ordinal();
        let growth = self.growth.unwrap_or(1.0);
        let mut cost = self.initial_cost(guess, in_set, mass);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            let child: Vec<u32> = group.iter().map(|&(_, i)| i).collect();
            let child_allowed =
                self.pool
                    .allowed_after(allowed, self.guess_pool, guess, group[0].0);
            cost += self.solve_within(&child, &child_allowed).cost;
            // costs only grow from here, so this guess can no longer be as cheap
            if best.is_some_and(|b| cost * growth > b.cost) {
                return;
            }
        }
        let cost = cost * growth;
        let ahead = |b: Solved| {
            // candidates rank ahead of non-candidates, then pool (frequency) order
            let rank = |g: u32| (set.binary_search(&g).is_err(), g);
            match cost.total_cmp(&b.cost) {
                Ordering::Less => true,
                Ordering::Equal => rank(guess) < rank(b.guess),
                Ordering::Greater => false,
            }
        };
        if best.is_none_or(ahead) {
            *best = Some(Solved { cost, guess });
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "bench")]
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "bench functions are named for the divan report, not documented; clippy skips them in the test harness build"
    )
)]
mod benches;
