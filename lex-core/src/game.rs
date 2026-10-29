//! Playing one game to completion.

use lex_data::Word;

use crate::correctness::WordCorrectness;
use crate::guesser::{Guess, Guesser};

/// The outcome of one game: the answer and the guesses that found it.
pub struct GameResult<const N: usize> {
    /// The answer.
    word: Word<N>,
    /// The guesses played, the last of them the answer.
    guesses: Vec<Guess<N>>,
}

impl<const N: usize> GameResult<N> {
    /// The answer.
    pub fn word(&self) -> Word<N> {
        self.word
    }

    /// The guesses played, in order.
    pub fn guesses(&self) -> &[Guess<N>] {
        &self.guesses
    }

    /// How many guesses the game took.
    pub fn num_guesses(&self) -> usize {
        self.guesses.len()
    }
}

/// Plays one game to completion from `history`, resetting `guesser` first. Reusing one
/// `guesser` across games shares its decision tree, so no position is scored twice.
pub fn play<const N: usize>(
    word: Word<N>,
    guesser: &mut Guesser<N>,
    history: Vec<Guess<N>>,
) -> anyhow::Result<GameResult<N>> {
    guesser.reset();
    for guess in history {
        guesser.push_guess(guess);
    }

    for (guess_num, guess) in guesser.history().iter().enumerate() {
        log_guess(guess_num + 1, guess);
    }

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
        log_guess(guess_num, &guess);

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

/// Logs one guess and its feedback at `info`.
#[inline(always)]
fn log_guess<const N: usize>(guess_num: usize, guess: &Guess<N>) {
    log::info!(
        "Guess {guess_num}: {} -> {}",
        guess.word(),
        guess.correctness()
    );
}
