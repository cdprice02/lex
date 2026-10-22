use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use lex_data::{Word, WordSet};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Chooses the words to play as targets.
///
/// With `file`, reads one word per line (blank lines ignored, case-folded) and requires each
/// to be in the dictionary, since the solver can only ever guess dictionary words. Otherwise
/// samples `num_games` distinct words uniformly at random from the dictionary, seeded for
/// reproducibility; `None` plays the whole dictionary.
pub fn select<const N: usize>(
    word_set: &WordSet<N>,
    file: Option<&Path>,
    num_games: Option<usize>,
    seed: u64,
) -> anyhow::Result<Vec<Word<N>>> {
    match file {
        Some(path) => from_file(word_set, path, num_games),
        None => Ok(sample(word_set, num_games, seed)),
    }
}

fn from_file<const N: usize>(
    word_set: &WordSet<N>,
    path: &Path,
    num_games: Option<usize>,
) -> anyhow::Result<Vec<Word<N>>> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading targets file {path:?}"))?;
    let dictionary: HashSet<Word<N>> = word_set.words().collect();
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let word = Word::<N>::try_from(line.to_lowercase().as_str())
            .with_context(|| format!("{path:?} line {}: {line:?}", i + 1))?;
        anyhow::ensure!(
            dictionary.contains(&word),
            "{path:?} line {}: {line:?} is not in the dictionary",
            i + 1
        );
        if seen.insert(word) {
            targets.push(word);
        }
    }
    anyhow::ensure!(!targets.is_empty(), "{path:?} contains no targets");
    if let Some(n) = num_games {
        targets.truncate(n);
    }
    Ok(targets)
}

fn sample<const N: usize>(
    word_set: &WordSet<N>,
    num_games: Option<usize>,
    seed: u64,
) -> Vec<Word<N>> {
    let mut words: Vec<Word<N>> = word_set.words().collect();
    let Some(n) = num_games else {
        return words;
    };
    if n >= words.len() {
        if n > words.len() {
            log::warn!(
                "dictionary length ({}) is less than number of games ({n}); playing each word once",
                words.len()
            );
        }
        return words;
    }
    // Partial Fisher-Yates over a ChaCha8 stream: uniform without replacement, and
    // reproducible from the seed alone.
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    for i in 0..n {
        let j = rng.random_range(i..words.len());
        words.swap(i, j);
    }
    words.truncate(n);
    words
}
