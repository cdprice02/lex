//! Lower bounds on a guess's cost, and the order they put the guesses in.

use std::cmp::{Ordering, Reverse};

use super::{Allowed, Endgame, PatternTable};

/// A guess's place in the try order: floor, then candidates before outside guesses, then pool
/// index. The index makes the order total, so any way of drawing it gives the same sequence.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ranked(pub(super) f64, pub(super) bool, pub(super) u32);

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
pub(super) const SLACK: f64 = 1e-9;

impl Endgame {
    /// Every allowed guess that may be tried, with its floor.
    pub(super) fn floors(
        &self,
        table: &PatternTable,
        set: &[u32],
        allowed: &Allowed,
        mass: f64,
    ) -> Vec<Reverse<Ranked>> {
        let mut keyed = Vec::with_capacity(set.len());
        let mut order = Vec::new();
        let cols = Self::columns(table, set);
        let members: Vec<(usize, f64)> = cols.as_deref().map_or_else(Vec::new, |cols| {
            cols.iter()
                .zip(set)
                .map(|(&col, &answer)| (col, self.weight(answer)))
                .collect()
        });
        let mut groups = vec![(0.0, 0.0, false); table.set.len()];
        let mut consider = |g: u32, in_set: bool| {
            let row = cols.as_ref().and_then(|_| table.row_of(g));
            let floor = row.map_or_else(
                || {
                    self.partition(table, cols.as_deref(), set, g, &mut keyed);
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
    pub(super) fn outside_cannot_win(best: f64, mass: f64) -> bool {
        best <= 2.0 * mass * (1.0 - SLACK)
    }

    /// A lower bound on the cost of `guess`, or `None` if it should not be tried. A non-candidate
    /// that cannot tell the answers apart only delays the game, and skipping it is what guarantees
    /// the recursion shrinks the set.
    ///
    /// A group of unsolved answers needs at least one guess for the one it names and two for every
    /// other (one more when only plain expectation counts), which is exact for a lone answer.
    pub(super) fn floor(
        &self,
        keyed: &[(u64, u32)],
        guess: u32,
        in_set: bool,
        mass: f64,
    ) -> Option<f64> {
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
    pub(super) fn group_floor(&self, group_mass: f64, top: f64) -> f64 {
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
    pub(super) fn floor_from_ranks(
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
}
