use std::path::Path;

use lex_data::Language;
use lex_data::Word;

use crate::guesser::correctness::WordCorrectness;
use crate::guesser::{Guess, Guesser};

pub struct GameResult<const N: usize> {
    word: Word<N>,
    guesses: Vec<Guess<N>>,
}

impl<const N: usize> GameResult<N> {
    pub fn new(word: Word<N>) -> Self {
        Self {
            word,
            guesses: Vec::new(),
        }
    }

    pub fn add_guess(&mut self, guess: Guess<N>) {
        self.guesses.push(guess);
    }

    pub fn word(&self) -> Word<N> {
        self.word
    }

    pub fn num_guesses(&self) -> usize {
        self.guesses.len()
    }
}

pub fn play<const N: usize>(
    word: Word<N>,
    data_dir: &Path,
    lang: Language,
    dictionary_length: Option<usize>,
) -> anyhow::Result<GameResult<N>> {
    let mut result = GameResult::new(word);

    let mut guesser = Guesser::<N>::try_new(data_dir, lang, dictionary_length)?;
    // TODO: use guesser to get the first guess; right now it is too complex of a problem
    let first_word = if N == 5 && lang == Language::English {
        Word::<N>::try_from("trace").expect("trace is 5 letters")
    } else {
        todo!("use guesser to get the first guess; right now it is too complex of a problem")
    };
    let guess = Guess::<N>::new(first_word, WordCorrectness::correct(word, first_word));
    result.add_guess(guess.clone());
    log::info!("Guess 1: {} -> {}", guess.word(), guess.correctness());
    let mut is_correct = guess.correctness().is_correct();
    while !is_correct {
        let word = guesser.next_guess(result.guesses.clone());
        let guess = Guess::<N>::new(word, WordCorrectness::correct(result.word(), word));
        result.add_guess(guess.clone());
        log::info!(
            "Guess {}: {} -> {}",
            result.num_guesses(),
            guess.word(),
            guess.correctness()
        );
        is_correct = guess.correctness().is_correct();
    }

    Ok(result)
}
