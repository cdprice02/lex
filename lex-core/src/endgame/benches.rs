//! Benches for the Endgame table and floors.

use divan::Bencher;
use std::hint::black_box;

use super::{Allowed, Endgame, GuessPool, PatternTable};
use crate::candidates::synthetic_pool;
use crate::pool_ops::shared;

/// A fixed set of 10 answers spread over a 12000-word pool, the size of English-5's.
const SET: [u32; 10] = [3, 40, 211, 480, 1_007, 2_500, 4_096, 6_001, 8_888, 11_500];

/// Builds the table and reads every floor of the whole pool against a set of 10, as the first
/// scan of a top-level call does.
fn build_and_floors(b: Bencher, aversion: f64) {
    let endgame = Endgame::new(
        shared(&synthetic_pool::<5>(12_000)),
        GuessPool::All,
        1.0,
        aversion,
    );
    let mass: f64 = SET.iter().map(|&i| endgame.weight(i)).sum();
    let mut table = PatternTable::default();
    b.bench_local(|| {
        table.active = false;
        endgame.pool.fill_table(&mut table, &SET, &Allowed::Whole);
        black_box(endgame.floors(&table, &SET, &Allowed::Whole, mass))
    });
}

#[divan::bench]
fn table_floors_10_plain(b: Bencher) {
    build_and_floors(b, 0.0);
}

#[divan::bench]
fn table_floors_10_averse(b: Bencher) {
    build_and_floors(b, 0.3);
}

/// Every floor of the whole pool for a child set of `child` of the table's 10 answers: what
/// each scan below the top-level call does.
fn child_floors(b: Bencher, aversion: f64, child: &[u32]) {
    let endgame = Endgame::new(
        shared(&synthetic_pool::<5>(12_000)),
        GuessPool::All,
        1.0,
        aversion,
    );
    let mut table = PatternTable::default();
    endgame.pool.fill_table(&mut table, &SET, &Allowed::Whole);
    let mass: f64 = child.iter().map(|&i| endgame.weight(i)).sum();
    b.bench_local(|| black_box(endgame.floors(&table, child, &Allowed::Whole, mass)));
}

#[divan::bench]
fn floors_child_3_of_10_plain(b: Bencher) {
    child_floors(b, 0.0, &[SET[1], SET[4], SET[7]]);
}

#[divan::bench]
fn floors_child_3_of_10_averse(b: Bencher) {
    child_floors(b, 0.3, &[SET[1], SET[4], SET[7]]);
}

#[divan::bench]
fn floors_top_10_plain(b: Bencher) {
    child_floors(b, 0.0, &SET);
}

/// The table build alone for the whole pool against a set of `n` answers.
fn build_only<const N: usize>(b: Bencher, n: usize) {
    let endgame = Endgame::new(
        shared(&synthetic_pool::<N>(12_000)),
        GuessPool::All,
        1.0,
        0.0,
    );
    let set: Vec<u32> = (0..n).map(|i| u32::try_from(i * 97).unwrap_or(0)).collect();
    let mut table = PatternTable::default();
    b.bench_local(|| {
        table.active = false;
        endgame.pool.fill_table(&mut table, &set, &Allowed::Whole);
        black_box(&table);
    });
}

#[divan::bench(consts = [5, 9])]
fn table_build_10<const N: usize>(b: Bencher) {
    build_only::<N>(b, 10);
}

#[divan::bench(consts = [5, 9])]
fn table_build_100<const N: usize>(b: Bencher) {
    build_only::<N>(b, 100);
}
