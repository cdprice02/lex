use std::collections::HashMap;

use crate::word::Word;

// TODO: move file locations and/or make them configurable
static WORDLE_WORDS_FILE_PATH: &str = "../data/wordle_words.txt";
static WORDLE_DICTIONARY_FILE_PATH: &str = "../data/wordle_dictionary.txt";
static WORD_FREQUENCY_FILE_PATH: &str = "../data/word_frequency.csv";

pub fn load_wordle_words<const N: usize>() -> Vec<Word<N>> {
    let contents =
        std::fs::read_to_string(WORDLE_WORDS_FILE_PATH).expect("Failed to read Wordle words file");
    contents.lines().map(Word::<N>::from).collect()
}

pub fn load_wordle_dictionary<const N: usize>() -> Vec<Word<N>> {
    let contents = std::fs::read_to_string(WORDLE_DICTIONARY_FILE_PATH)
        .expect("Failed to read Wordle dictionary file");
    contents.lines().map(Word::<N>::from).collect()
}

pub fn load_word_frequencies<const N: usize>() -> HashMap<Word<N>, f64> {
    let contents = std::fs::read_to_string(WORD_FREQUENCY_FILE_PATH)
        .expect("Failed to read word frequency file");
    contents
        .lines()
        .filter_map(|line| {
            let (word, freq) = line.split_once(',')?;
            let word = Word::<N>::from(word);
            let freq = freq.parse().ok()?;

            Some((word, freq))
        })
        .collect()
}
