//! Exact search for Candidate sets at or below the Endgame threshold.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::sync::Arc;

use crate::allowed::{Allowed, AllowedIds};
use crate::pool_ops::PoolOps;
use crate::strategy::{GuessPool, Pool};
use crate::transposed::{BLOCK, Columns, Plan};

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

/// The most answers a table can hold: a rank is a `u8`.
const MAX_TABLE_COLUMNS: usize = 255;

/// Every pattern one top-level call needs, computed once. A row per guess the call allows, a
/// column per answer of its set; a cell is the rank of that answer's pattern among the row's
/// distinct patterns over the set, in ascending ordinal order. Ranks order as ordinals do, so a
/// partition read from the table has the groups of `Pool::partition` in the same order. Every
/// child set is a subset of the set and every child's allowed guesses a subset of the rows, so
/// the recursion below the call never computes a pattern.
#[derive(Debug, Default)]
pub struct PatternTable {
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
    /// The (ordinal, column) pairs of the row being built, sorted to assign ranks.
    scratch: Vec<(u64, usize)>,
}

impl PatternTable {
    /// Fills the table for `set` and the guesses `allowed` permits.
    pub(crate) fn build<const N: usize>(&mut self, pool: &Pool<N>, set: &[u32], allowed: &Allowed) {
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
        let columns = Columns::gather(set.iter().map(|&a| pool.code(a)), pool.alphabet_len());
        let mut block = [0_u64; BLOCK];
        for row in 0..rows {
            let guess = if self.whole {
                u32::try_from(row).unwrap_or(u32::MAX)
            } else {
                self.rows[row]
            };
            let code = pool.code(guess);
            let span = row * cols..(row + 1) * cols;
            self.scratch.clear();
            let plan = Plan::new(code);
            for k in 0..columns.blocks() {
                columns.ordinals(&plan, k, &mut block);
                let first = k * BLOCK;
                self.scratch.extend(
                    block
                        .iter()
                        .take(columns.block_len(k))
                        .enumerate()
                        .map(|(b, &ordinal)| (ordinal, first + b)),
                );
            }
            self.scratch.sort_unstable();
            let (ranks, ordinals) = (&mut self.ranks[span.clone()], &mut self.ordinals[span]);
            let mut rank = 0_usize;
            let mut previous = None;
            for &(ordinal, col) in &self.scratch {
                if previous.is_some_and(|p| p != ordinal) {
                    rank += 1;
                }
                previous = Some(ordinal);
                ranks[col] = u8::try_from(rank).unwrap_or(u8::MAX);
                ordinals[rank] = ordinal;
            }
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

    /// The ranks and ordinals of `guess`'s row, if the call allowed it.
    fn row_of(&self, guess: u32) -> Option<(&[u8], &[u64])> {
        let width = self.set.len();
        let span = self.row(guess).map(|row| row * width..(row + 1) * width)?;
        Some((self.ranks.get(span.clone())?, self.ordinals.get(span)?))
    }

    /// The table column of each member of `set` into `cols`, or false when a member is not a
    /// column. A set equal to the table's own maps by identity.
    fn columns(&self, set: &[u32], cols: &mut Vec<usize>) -> bool {
        cols.clear();
        if set == self.set {
            cols.extend(0..set.len());
            return true;
        }
        for answer in set {
            let Ok(col) = self.set.binary_search(answer) else {
                return false;
            };
            cols.push(col);
        }
        true
    }

    /// Fills `out` as `Pool::partition` would for `set`, whose table columns are `cols`, or
    /// returns false (leaving `out` unspecified) when the guess lies outside the table.
    fn partition(
        &self,
        cols: &[usize],
        set: &[u32],
        guess: u32,
        out: &mut Vec<(u64, u32)>,
    ) -> bool {
        let width = self.set.len();
        let Some(row) = self.row(guess) else {
            return false;
        };
        let span = row * width..(row + 1) * width;
        let (Some(ranks), Some(ordinals)) = (self.ranks.get(span.clone()), self.ordinals.get(span))
        else {
            return false;
        };
        // counting sort by rank: ranks are below the table's width, and placing in set order keeps
        // a group ascending by index
        let mut starts = [0_usize; MAX_TABLE_COLUMNS + 1];
        for &col in cols {
            starts[usize::from(ranks[col]) + 1] += 1;
        }
        for rank in 0..width {
            starts[rank + 1] += starts[rank];
        }
        out.clear();
        out.resize(set.len(), (0, 0));
        for (&col, &answer) in cols.iter().zip(set) {
            let rank = usize::from(ranks[col]);
            out[starts[rank]] = (ordinals[rank], answer);
            starts[rank] += 1;
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

    /// Every allowed guess that may be tried, with its floor.
    fn floors(&self, set: &[u32], allowed: &Allowed, mass: f64) -> Vec<Reverse<Ranked>> {
        let mut keyed = Vec::with_capacity(set.len());
        let mut order = Vec::new();
        let cols = self.columns(set);
        let members: Vec<(usize, f64)> = cols.as_deref().map_or_else(Vec::new, |cols| {
            cols.iter()
                .zip(set)
                .map(|(&col, &answer)| (col, self.weight(answer)))
                .collect()
        });
        let mut groups = vec![(0.0, 0.0, false); self.table.set.len()];
        let mut consider = |g: u32, in_set: bool| {
            let row = cols.as_ref().and_then(|_| self.table.row_of(g));
            let floor = row.map_or_else(
                || {
                    self.partition(cols.as_deref(), set, g, &mut keyed);
                    self.floor(&keyed, g, in_set, mass)
                },
                |row| self.floor_from_ranks(&mut groups, &members, row, g, in_set, mass),
            );
            if let Some(floor) = floor {
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
        let win = self.pool.win_ordinal();
        let mut sum = self.initial_cost(guess, in_set, mass);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if group[0].0 == win {
                continue;
            }
            let weights = group.iter().map(|&(_, i)| self.weight(i));
            let (group_mass, top) = weights.fold((0.0, 0.0_f64), |(m, t), w| (m + w, t.max(w)));
            sum += self.group_floor(group_mass, top);
        }
        Some(sum * self.growth.unwrap_or(1.0))
    }

    /// What an unsolved group of total weight `group_mass` and heaviest answer `top` adds to a floor.
    fn group_floor(&self, group_mass: f64, top: f64) -> f64 {
        self.growth.map_or_else(
            || 2.0_f64.mul_add(group_mass, -top),
            |a| (a * a).mul_add(group_mass - top, a * top),
        )
    }

    /// [`Self::floor`] read straight from a table row, without building the partition: `members`
    /// holds the table column and weight of each member of the set, and `row` the ranks and
    /// ordinals of `guess`. Each rank accumulates its weights in set order, as its group of the
    /// sorted partition does, and the groups are added in ascending rank, as `floor` adds them,
    /// so the sums are the same bits.
    /// `groups` is scratch with a slot per table column.
    fn floor_from_ranks(
        &self,
        groups: &mut [(f64, f64, bool)],
        members: &[(usize, f64)],
        row: (&[u8], &[u64]),
        guess: u32,
        in_set: bool,
        mass: f64,
    ) -> Option<f64> {
        let (ranks, ordinals) = row;
        groups.fill((0.0, 0.0, false));
        for &(col, w) in members {
            let slot = &mut groups[usize::from(ranks[col])];
            *slot = (slot.0 + w, slot.1.max(w), true);
        }
        if !in_set && groups.iter().filter(|g| g.2).count() <= 1 {
            return None;
        }
        let win = self.pool.win_ordinal();
        let mut sum = self.initial_cost(guess, in_set, mass);
        for (&(group_mass, top, present), &ordinal) in groups.iter().zip(ordinals) {
            if present && ordinal != win {
                sum += self.group_floor(group_mass, top);
            }
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
