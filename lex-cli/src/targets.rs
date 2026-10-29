//! Choosing the target words a simulation plays.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use lex_data::Word;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Chooses the words to play as targets.
///
/// `answers` is every word a game can be about (the Answer list, or the whole dictionary
/// without a Profile); `noun` names it in errors.
///
/// With `file`, reads one word per line (blank lines ignored, case-folded) and requires each
/// to be in `answers`. Otherwise samples `num_games` distinct words uniformly at random from
/// `answers`, seeded for reproducibility; `None` plays all of them.
pub fn select<const N: usize>(
    answers: Vec<Word<N>>,
    noun: &str,
    file: Option<&Path>,
    num_games: Option<usize>,
    seed: u64,
) -> anyhow::Result<Vec<Word<N>>> {
    match file {
        Some(path) => from_file(&answers, noun, path, num_games),
        None => Ok(sample(answers, num_games, seed)),
    }
}

/// The distinct words in the file at `path`, each required to be an answer, at most `num_games` of them.
fn from_file<const N: usize>(
    answers: &[Word<N>],
    noun: &str,
    path: &Path,
    num_games: Option<usize>,
) -> anyhow::Result<Vec<Word<N>>> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading targets file {path:?}"))?;
    let dictionary: HashSet<Word<N>> = answers.iter().copied().collect();
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
            "{path:?} line {}: {line:?} is not in {noun}",
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

/// `num_games` words of `words` drawn uniformly, reproducible from `seed`; all of them when `None`.
fn sample<const N: usize>(
    mut words: Vec<Word<N>>,
    num_games: Option<usize>,
    seed: u64,
) -> Vec<Word<N>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<Word<5>> {
        list.iter().map(|w| Word::try_from(*w).unwrap()).collect()
    }

    #[test]
    fn samples_only_from_the_given_words() {
        let answers = words(&["crane", "stare", "light", "mount", "swipe"]);
        let picked = select(answers.clone(), "the dictionary", None, Some(3), 7).unwrap();
        assert_eq!(picked.len(), 3);
        assert!(picked.iter().all(|w| answers.contains(w)));
        assert_eq!(
            picked,
            select(answers, "the dictionary", None, Some(3), 7).unwrap()
        );
    }

    #[test]
    fn targets_file_words_must_be_answers() {
        let dir = std::env::temp_dir().join(format!("lex-targets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.txt");
        std::fs::write(&file, "crane\nzebra\n").unwrap();
        let err = select(
            words(&["crane", "stare"]),
            "the wordle answer list",
            Some(&file),
            None,
            0,
        )
        .unwrap_err();
        assert!(err.to_string().contains("is not in the wordle answer list"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
