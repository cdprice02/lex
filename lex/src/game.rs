use crate::guesser::correctness::WordCorrectness;
use crate::guesser::{Guess, Guesser};
use crate::word::Word;

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

pub fn play<const N: usize>(word: Word<N>) -> GameResult<N> {
    let mut result = GameResult::new(word);

    let mut guesser = Guesser::<N>::new();
    let first_word = Word::from("trace"); // TODO: use guesser to get the first guess; right now it is too complex of a problem
    let guess = Guess::<N>::new(first_word, WordCorrectness::correct(word, first_word));
    result.add_guess(guess);
    eprintln!(
        "Guess 1: {} -> {}",
        result.guesses[0].word(),
        result.guesses[0].correctness()
    );
    let mut is_correct = result
        .guesses
        .last()
        .expect("just added guess")
        .correctness()
        .is_correct();
    while !is_correct {
        let word = guesser.next_guess(result.guesses.clone());
        let guess = Guess::<N>::new(word, WordCorrectness::correct(result.word(), word));
        result.add_guess(guess);
        eprintln!(
            "Guess {}: {} -> {}",
            result.num_guesses(),
            result.guesses.last().unwrap().word(),
            result.guesses.last().unwrap().correctness()
        );
        is_correct = result
            .guesses
            .last()
            .expect("just added guess")
            .correctness()
            .is_correct();
    }

    result
}
