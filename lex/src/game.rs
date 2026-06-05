use lex_data::Word;
use lex_data::WordSet;

use crate::guesser::correctness::WordCorrectness;
use crate::guesser::{Guess, Guesser};

pub struct GameResult<const N: usize> {
    word: Word<N>,
    guesses: Vec<Guess<N>>,
}

impl<const N: usize> GameResult<N> {
    pub fn word(&self) -> Word<N> {
        self.word
    }

    pub fn num_guesses(&self) -> usize {
        self.guesses.len()
    }
}

pub fn play<const N: usize>(
    word: Word<N>,
    word_set: &WordSet<N>,
    history: Vec<Guess<N>>,
) -> anyhow::Result<GameResult<N>> {
    let mut guesser = Guesser::with_history(word_set.clone(), history);

    if guesser
        .history()
        .last()
        .is_some_and(|g| g.correctness().is_correct())
    {
        return Ok(GameResult {
            word,
            guesses: guesser.history().to_vec(),
        });
    }

    let mut guess_num = guesser.history().len() + 1;
    loop {
        let suggestion = guesser
            .suggest()
            .ok_or_else(|| anyhow::anyhow!("no valid words remain in dictionary"))?;

        let correctness = WordCorrectness::correct(word, suggestion);
        let guess = Guess::new(suggestion, correctness);
        log::info!("Guess {}: {} -> {}", guess_num, suggestion, correctness);

        guesser.push_guess(guess);

        if correctness.is_correct() {
            break;
        }

        guess_num += 1;
    }

    Ok(GameResult {
        word,
        guesses: guesser.history().to_vec(),
    })
}
