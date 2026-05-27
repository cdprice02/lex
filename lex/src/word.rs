use crate::guesser::Guess;
use crate::guesser::correctness::WordCorrectness;
use std::{fmt::Display, ops::Deref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Word<const N: usize>([char; N]);

impl<const N: usize> Word<N> {
    pub fn matches(&self, guess: &Guess<N>) -> bool {
        let correctness = WordCorrectness::correct(*self, guess.word());
        correctness == guess.correctness()
    }
}

impl<const N: usize> From<String> for Word<N> {
    fn from(s: String) -> Self {
        Word::from(s.as_str())
    }
}

impl<const N: usize> From<&str> for Word<N> {
    fn from(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        assert!(
            chars.len() == N,
            "input string must have exactly {} characters",
            N
        );
        let array: [char; N] = chars.try_into().expect("length checked above");
        Word(array)
    }
}

impl<const N: usize> Deref for Word<N> {
    type Target = [char; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> Display for Word<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ch in &self.0 {
            write!(f, "{}", ch)?;
        }
        Ok(())
    }
}

impl<const N: usize> IntoIterator for Word<N> {
    type Item = char;
    type IntoIter = std::array::IntoIter<char, N>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterator::into_iter(*self)
    }
}
