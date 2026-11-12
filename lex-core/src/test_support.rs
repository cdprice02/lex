//! Randomized helpers shared by lex-core tests.

use std::sync::Arc;

use lex_data::{Word, WordSet};

use crate::strategy::Pool;

/// A deterministic xorshift stream, so a failing round replays.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }

    pub fn index(&mut self, n: usize) -> u32 {
        u32::try_from(self.below(n)).unwrap()
    }
}

/// A pool of distinct words over a four-letter alphabet, so patterns collide often. By
/// `shape`, counts differ mildly, repeat (exact weight ties), or span nine orders of magnitude
/// (tiny weights).
pub fn random_pool(rng: &mut Rng, size: usize, shape: usize) -> Arc<Pool<5>> {
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

pub fn random_candidates(rng: &mut Rng, pool: &Pool<5>, len: usize) -> Vec<u32> {
    let mut set = std::collections::BTreeSet::new();
    while set.len() < len {
        set.insert(rng.index(pool.len()));
    }
    set.into_iter().collect()
}

/// The `len` words nearest a random seed word by letters differing, which are hard to tell
/// apart and so make guesses outside the set worth scanning for.
pub fn similar_candidates(rng: &mut Rng, pool: &Pool<5>, len: usize) -> Vec<u32> {
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
