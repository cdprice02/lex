//! Exact search for Candidate sets at or below the Endgame threshold.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
use lex_data::Word;

use crate::allowed::{Allowed, AllowedIds};
use crate::correctness::{LetterCounts, WordCorrectness, pattern_ordinal};
use crate::strategy::{GuessPool, Pool};

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
pub struct Endgame<const N: usize> {
    /// The Guess pool the set indices point into.
    pool: Arc<Pool<N>>,
    /// The Strategy's Guess pool, which decides which guesses a position allows.
    guess_pool: GuessPool,
    /// `a` in the `a^guesses` objective; `None` is plain expected guesses (the `a -> 1` limit).
    growth: Option<f64>,
    /// Answer prior weight of each pool word, unnormalized.
    weights: Vec<f64>,
    /// Each distinct allowed-guess list met so far, numbered, so a memo key is small.
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

/// The most answers a table can hold: a rank is a `u8`.
const MAX_TABLE_COLUMNS: usize = 255;

/// Every pattern one top-level call needs, computed once. A row per guess the call allows, a
/// column per answer of its set; a cell is the rank of that answer's pattern among the row's
/// distinct patterns over the set, in ascending ordinal order. Ranks order as ordinals do, so a
/// partition read from the table has the groups of `Pool::partition` in the same order. Every
/// child set is a subset of the set and every child's allowed guesses a subset of the rows, so
/// the recursion below the call never computes a pattern.
#[derive(Debug, Default)]
struct PatternTable {
    /// Whether the table describes the call in progress.
    active: bool,
    /// The top-level set (ascending pool indices): the columns.
    set: Vec<u32>,
    /// Whether the rows are every pool word, in pool order; otherwise `rows` lists them.
    whole: bool,
    /// The guess of each row (ascending), when not `whole`.
    rows: Vec<u32>,
    /// `rows * set.len()` ranks, row-major.
    ranks: Vec<u8>,
    /// Per row, the distinct ordinals in ascending order, indexed by rank (the rest is unused).
    /// The all-green ordinal, when the row has it, is the last one: the win rank.
    ordinals: Vec<u64>,
    /// Sort buffer for building one row.
    scratch: Vec<u64>,
}

impl PatternTable {
    /// Fills the table for `set` and the guesses `allowed` permits.
    fn build<const N: usize>(&mut self, pool: &Pool<N>, set: &[u32], allowed: &Allowed) {
        let cols = set.len();
        self.set.clear();
        self.set.extend_from_slice(set);
        self.rows.clear();
        self.whole = matches!(allowed, Allowed::Whole);
        match allowed {
            Allowed::Whole => {}
            Allowed::List(list) => self.rows.extend_from_slice(list),
            Allowed::Candidates => self.rows.extend_from_slice(set),
        }
        let rows = if self.whole {
            pool.len()
        } else {
            self.rows.len()
        };
        self.ranks.clear();
        self.ranks.resize(rows * cols, 0);
        self.ordinals.clear();
        self.ordinals.resize(rows * cols, 0);
        let mut counts: LetterCounts = [0; 256];
        for row in 0..rows {
            let guess = if self.whole {
                u32::try_from(row).unwrap_or(u32::MAX)
            } else {
                self.rows[row]
            };
            let code = pool.code(guess);
            let span = row * cols..(row + 1) * cols;
            let ordinals = &mut self.ordinals[span.clone()];
            for (o, &answer) in ordinals.iter_mut().zip(set) {
                *o = pattern_ordinal(pool.code(answer), code, &mut counts);
            }
            self.scratch.clear();
            self.scratch.extend_from_slice(ordinals);
            self.scratch.sort_unstable();
            self.scratch.dedup();
            for (rank, o) in self.ranks[span].iter_mut().zip(ordinals.iter()) {
                let place = self.scratch.binary_search(o).unwrap_or_default();
                *rank = u8::try_from(place).unwrap_or(u8::MAX);
            }
            ordinals[..self.scratch.len()].copy_from_slice(&self.scratch);
        }
        self.active = true;
    }

    /// The row of `guess`, if the call allowed it.
    fn row(&self, guess: u32) -> Option<usize> {
        if self.whole {
            usize::try_from(guess).ok()
        } else {
            self.rows.binary_search(&guess).ok()
        }
    }

    /// Fills `out` as `Pool::partition` would, or returns false (leaving `out` unspecified) when
    /// the guess or an answer lies outside the table.
    fn partition(&self, set: &[u32], guess: u32, out: &mut Vec<(u64, u32)>) -> bool {
        let cols = self.set.len();
        let Some(row) = self.row(guess) else {
            return false;
        };
        let span = row * cols..(row + 1) * cols;
        let (Some(ranks), Some(ordinals)) = (self.ranks.get(span.clone()), self.ordinals.get(span))
        else {
            return false;
        };
        out.clear();
        for &answer in set {
            let Ok(col) = self.set.binary_search(&answer) else {
                return false;
            };
            out.push((u64::from(ranks[col]), answer));
        }
        // stable, so a group stays ascending by index
        out.sort_by_key(|&(rank, _)| rank);
        for entry in out.iter_mut() {
            entry.0 = ordinals[usize::try_from(entry.0).unwrap_or(usize::MAX)];
        }
        true
    }
}

/// A guess's place in the try order: floor, then candidates before outside guesses, then pool
/// index. The index makes the order total, so any way of drawing it gives the same sequence.
#[derive(Debug, Clone, Copy)]
struct Ranked(f64, bool, u32);

impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Ranked {}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .total_cmp(&other.0)
            .then(self.1.cmp(&other.1))
            .then(self.2.cmp(&other.2))
    }
}

/// Relative slack on the bounds that skip a scan. A cost is a short float sum, off by about 1e-15
/// of its value; this is far above that, so a skip fires only when no rounding can change the
/// winner, and anything closer is scanned.
const SLACK: f64 = 1e-9;

impl<const N: usize> Endgame<N> {
    /// An empty search over `pool`, weighting answers by the Answer prior at `temperature` and scoring risk by `aversion`.
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
            list_ids: AllowedIds::default(),
            memo: HashMap::new(),
            table: PatternTable::default(),
            #[cfg(test)]
            reference: false,
        }
    }

    /// Numbers allowed-guess lists in `ids`, a table another memo may share.
    pub(crate) fn sharing(mut self, ids: AllowedIds) -> Self {
        self.list_ids = ids;
        self
    }

    /// Fills `out` with `set` against `guess` as [`Pool::partition`] does, from the table when
    /// one is active. A guess or answer outside it breaks the subset invariant and is computed
    /// directly in release builds.
    fn partition(&self, set: &[u32], guess: u32, out: &mut Vec<(u64, u32)>) {
        if self.table.active {
            if self.table.partition(set, guess, out) {
                return;
            }
            debug_assert!(false, "guess {guess} or set {set:?} is outside the table");
        }
        self.pool.partition(set, guess, out);
    }

    /// The Answer prior weight of the pool word at `index`.
    fn weight(&self, index: u32) -> f64 {
        self.weights[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// The guess minimizing expected guesses for the answers in `set` (ascending pool indices).
    #[cfg(test)]
    pub(crate) fn best_guess(&mut self, set: &[u32], allowed: &Allowed) -> Option<Word<N>> {
        self.best_index(set, allowed).map(|i| self.pool.word(i))
    }

    /// The guess of `best_guess` as a pool index.
    pub(crate) fn best_index(&mut self, set: &[u32], allowed: &Allowed) -> Option<u32> {
        if set.is_empty() {
            return None;
        }
        Some(self.solve(set, allowed).guess)
    }

    /// Expected number of guesses to finish from `set` under the answer prior renormalized to it.
    #[cfg(test)]
    pub(crate) fn expected_guesses(&mut self, set: &[u32], allowed: &Allowed) -> f64 {
        let mass: f64 = set.iter().map(|&i| self.weight(i)).sum();
        self.solve(set, allowed).cost / mass
    }

    /// The best guess and cost for `set`, memoized: the top-level entry. On a miss it builds the
    /// pattern table once for the whole search below it; the recursion goes through
    /// [`Self::solve_within`], so it never rebuilds the table and no entry nests in another.
    fn solve(&mut self, set: &[u32], allowed: &Allowed) -> Solved {
        let slot = self.list_ids.slot(allowed);
        if let Some(&solved) = self.memo.get(&slot).and_then(|m| m.get(set)) {
            return solved;
        }
        #[cfg(test)]
        let tabled = !self.reference;
        #[cfg(not(test))]
        let tabled = true;
        if tabled && (2..=MAX_TABLE_COLUMNS).contains(&set.len()) {
            let mut table = std::mem::take(&mut self.table);
            table.build(&self.pool, set, allowed);
            self.table = table;
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

    /// Every allowed guess that may be tried, with its floor.
    fn floors(&self, set: &[u32], allowed: &Allowed, mass: f64) -> Vec<Reverse<Ranked>> {
        let mut keyed = Vec::with_capacity(set.len());
        let mut order = Vec::new();
        let mut consider = |g: u32, in_set: bool| {
            self.partition(set, g, &mut keyed);
            if let Some(floor) = self.floor(&keyed, g, in_set, mass) {
                order.push(Reverse(Ranked(floor, !in_set, g)));
            }
        };
        for &g in set {
            consider(g, true);
        }
        let mut beyond = |guesses: &mut dyn Iterator<Item = u32>| {
            let mut next = 0;
            for g in guesses {
                if set.get(next) == Some(&g) {
                    next += 1;
                } else {
                    consider(g, false);
                }
            }
        };
        match allowed {
            Allowed::Whole => beyond(&mut self.pool.indices()),
            Allowed::List(list) => beyond(&mut list.iter().copied()),
            Allowed::Candidates => {}
        }
        order
    }

    /// Whether no guess outside the set can beat a candidate costing `best` in plain mode, so
    /// the pool need not be scanned. Outside the set every answer survives this turn, so each
    /// takes two guesses or more: `2 * mass`. Candidates rank ahead on an exact tie, so only a
    /// strictly lower cost matters, and [`SLACK`] covers float rounding.
    fn outside_cannot_win(best: f64, mass: f64) -> bool {
        best <= 2.0 * mass * (1.0 - SLACK)
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
        let win = WordCorrectness::<N>::COUNT - 1;
        let mut keyed = Vec::with_capacity(set.len());
        let mut best = Solved {
            cost: f64::INFINITY,
            guess: set[0],
        };
        for &guess in set {
            self.partition(set, guess, &mut keyed);
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
            let weights = group.iter().map(|&(_, i)| self.weight(i));
            let (group_mass, top) = weights.fold((0.0, 0.0_f64), |(m, t), w| (m + w, t.max(w)));
            sum += self.growth.map_or_else(
                || 2.0_f64.mul_add(group_mass, -top),
                |a| (a * a).mul_add(group_mass - top, a * top),
            );
        }
        Some(sum * self.growth.unwrap_or(1.0))
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
        self.partition(set, guess, &mut keyed);

        let win = WordCorrectness::<N>::COUNT - 1;
        let growth = self.growth.unwrap_or(1.0);
        let mut cost = self.initial_cost(guess, in_set, mass);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            let child: Vec<u32> = group.iter().map(|&(_, i)| i).collect();
            let child_allowed = allowed.after(
                self.guess_pool,
                &self.pool,
                self.pool.code(guess),
                group[0].0,
            );
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
mod tests {
    use lex_data::WordSet;

    use super::*;
    use crate::correctness::pattern_ordinal;

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
    fn simulated_cost(endgame: &mut Endgame<5>, set: &[u32]) -> f64 {
        let mut total = 0.0;
        for &answer in set {
            let target = endgame.pool.word(answer);
            let mut remaining = set.to_vec();
            let mut guesses = 0;
            loop {
                let guess = endgame.best_guess(&remaining, &Allowed::Whole).unwrap();
                guesses += 1;
                if guess == target {
                    break;
                }
                let feedback = ordinal_of(&endgame.pool, target, guess);
                remaining.retain(|&i| {
                    ordinal_of(&endgame.pool, endgame.pool.word(i), guess) == feedback
                });
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
            let mut endgame = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, aversion);
            let predicted = endgame.solve(&set, &Allowed::Whole).cost;
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
                        let guess = endgame.best_guess(&remaining, &Allowed::Whole).unwrap();
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
        let all = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, 0.0)
            .expected_guesses(&set, &Allowed::Whole);
        let hard = Endgame::new(Arc::clone(&pool), GuessPool::Ultrahard, 5.0, 0.0)
            .expected_guesses(&set, &Allowed::Candidates);
        assert!(all < hard, "{all} !< {hard}");
    }

    #[test]
    fn the_memo_never_reuses_a_cost_across_different_allowed_guesses() {
        let pool = pool_of(&[&FAMILY[..], &["clamp", "twang", "phlox"]].concat());
        let set = indices_of(&pool, &FAMILY);
        let restricted = Allowed::List(set.clone());
        let mut endgame = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, 0.0);
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
        let fresh = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, 0.0)
            .expected_guesses(&set, &restricted);
        assert_eq!(limited.to_bits(), fresh.to_bits());
    }

    #[test]
    fn a_lone_candidate_is_one_guess() {
        let pool = pool_of(&FAMILY);
        let mut endgame = Endgame::new(Arc::clone(&pool), GuessPool::All, 5.0, 0.0);
        assert_eq!(
            endgame.expected_guesses(&[3], &Allowed::Whole).to_bits(),
            1.0_f64.to_bits()
        );
        assert_eq!(
            endgame.best_guess(&[3], &Allowed::Whole),
            Some(pool.word(3))
        );
        assert_eq!(endgame.best_guess(&[], &Allowed::Whole), None);
    }

    #[test]
    fn two_candidates_cost_one_and_a_half_guesses_when_equally_likely() {
        let pool = Pool::new(&WordSet::from_pairs(vec![
            (Word::<5>::try_from("crane").unwrap(), 10),
            (Word::<5>::try_from("crate").unwrap(), 10),
        ]));
        let mut endgame = Endgame::new(pool, GuessPool::All, 5.0, 0.0);
        assert!((endgame.expected_guesses(&[0, 1], &Allowed::Whole) - 1.5).abs() < 1e-12);
    }

    /// A deterministic xorshift stream, so a failing round replays.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
        }

        fn index(&mut self, n: usize) -> u32 {
            u32::try_from(self.below(n)).unwrap()
        }
    }

    /// A pool of distinct words over a four-letter alphabet, so patterns collide often. By
    /// `shape`, counts differ mildly, repeat (exact weight ties), or span nine orders of magnitude
    /// (tiny weights).
    fn random_pool(rng: &mut Rng, size: usize, shape: usize) -> Arc<Pool<5>> {
        let mut seen = std::collections::HashSet::new();
        let mut pairs = Vec::new();
        while pairs.len() < size {
            let text: String = (0..5)
                .map(|_| char::from(b'a' + u8::try_from(rng.below(4)).unwrap()))
                .collect();
            if seen.insert(text.clone()) {
                let count = match shape {
                    0 => 1000 + rng.next() % 5000,
                    1 => 1 + rng.next() % 3,
                    _ => 10_u64.pow(u32::try_from(rng.below(10)).unwrap()),
                };
                pairs.push((Word::<5>::try_from(text.as_str()).unwrap(), count));
            }
        }
        Pool::new(&WordSet::from_pairs(pairs))
    }

    fn random_set(rng: &mut Rng, pool: &Pool<5>, len: usize) -> Vec<u32> {
        let mut set = std::collections::BTreeSet::new();
        while set.len() < len {
            set.insert(rng.index(pool.len()));
        }
        set.into_iter().collect()
    }

    /// The `len` words nearest a random seed word by letters differing, which are hard to tell
    /// apart and so make guesses outside the set worth scanning for.
    fn similar_set(rng: &mut Rng, pool: &Pool<5>, len: usize) -> Vec<u32> {
        let seed = pool.code(rng.index(pool.len()));
        let distance = |i: u32| {
            pool.code(i)
                .iter()
                .zip(seed.iter())
                .filter(|(a, b)| a != b)
                .count()
        };
        let mut keyed: Vec<(usize, u64, u32)> = pool
            .indices()
            .map(|i| (distance(i), rng.next(), i))
            .collect();
        keyed.sort_unstable();
        let mut all: Vec<u32> = keyed.iter().take(len).map(|k| k.2).collect();
        all.sort_unstable();
        all
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
        let mut fast = Endgame::new(Arc::clone(pool), mode, temperature, aversion);
        let mut full = Endgame::new(Arc::clone(pool), mode, temperature, aversion);
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
                        random_set(&mut rng, &pool, len)
                    } else {
                        similar_set(&mut rng, &pool, len)
                    };
                    // the list holds every candidate, as the search requires
                    let mut list = set.clone();
                    let extra = 3 + rng.below(12);
                    list.extend(random_set(&mut rng, &pool, extra));
                    list.sort_unstable();
                    list.dedup();
                    for (mode, allowed) in [
                        (GuessPool::All, Allowed::Whole),
                        (GuessPool::All, Allowed::List(list)),
                        (GuessPool::Ultrahard, Allowed::Candidates),
                    ] {
                        assert_matches_reference(
                            &pool,
                            mode,
                            temperature,
                            aversion,
                            &set,
                            &allowed,
                        );
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
            let set = random_set(&mut rng, &pool, len);
            let mut list = set.clone();
            let extra = 3 + rng.below(20);
            list.extend(random_set(&mut rng, &pool, extra));
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
                        assert!(table.partition(part, guess, &mut tabled));
                        assert_eq!(direct, tabled, "guess {guess} over {part:?}");
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
        let mut endgame =
            Endgame::new(Arc::clone(&pool), GuessPool::All, strategy.temperature, 0.4);
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
