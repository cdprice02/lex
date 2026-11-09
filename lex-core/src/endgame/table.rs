//! The pattern table of one top-level Endgame call: every (guess, answer) feedback, ranked.

use crate::allowed::Allowed;
use crate::strategy::Pool;
use crate::transposed::{BLOCK, Columns, Plan};

/// The most answers a table can hold: a rank is a `u8`.
pub(super) const MAX_TABLE_COLUMNS: usize = 255;

/// Every pattern one top-level call needs, computed once. A row per guess the call allows, a
/// column per answer of its set; a cell is the rank of that answer's pattern among the row's
/// distinct patterns over the set, in ascending ordinal order. Ranks order as ordinals do, so a
/// partition read from the table has the groups of `Pool::partition` in the same order. Every
/// child set is a subset of the set and every child's allowed guesses a subset of the rows, so
/// the recursion below the call never computes a pattern.
#[derive(Debug, Default)]
pub struct PatternTable {
    /// Whether the table describes the call in progress.
    pub(super) active: bool,
    /// The top-level set (ascending pool indices): the columns.
    pub(super) set: Vec<u32>,
    /// Whether the rows are every pool word, in pool order; otherwise `rows` lists them.
    pub(super) whole: bool,
    /// The guess of each row (ascending), when not `whole`.
    pub(super) rows: Vec<u32>,
    /// `rows * set.len()` ranks, row-major.
    pub(super) ranks: Vec<u8>,
    /// Per row, the distinct ordinals in ascending order, indexed by rank (the rest is unused).
    /// The all-green ordinal, when the row has it, is the last one: the win rank.
    pub(super) ordinals: Vec<u64>,
    /// The (ordinal, column) pairs of the row being built, sorted to assign ranks.
    pub(super) scratch: Vec<(u64, usize)>,
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
    pub(super) fn row(&self, guess: u32) -> Option<usize> {
        if self.whole {
            usize::try_from(guess).ok()
        } else {
            self.rows.binary_search(&guess).ok()
        }
    }

    /// The ranks and ordinals of `guess`'s row, if the call allowed it.
    pub(super) fn row_of(&self, guess: u32) -> Option<(&[u8], &[u64])> {
        let width = self.set.len();
        let span = self.row(guess).map(|row| row * width..(row + 1) * width)?;
        Some((self.ranks.get(span.clone())?, self.ordinals.get(span)?))
    }

    /// The table column of each member of `set` into `cols`, or false when a member is not a
    /// column. A set equal to the table's own maps by identity.
    pub(super) fn columns(&self, set: &[u32], cols: &mut Vec<usize>) -> bool {
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
    pub(super) fn partition(
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
