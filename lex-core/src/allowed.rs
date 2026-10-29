//! The guesses a game position permits, including the hard-mode rule.

use crate::cast;
use std::fmt;

use crate::correctness::{Code, LetterCounts, pattern_ordinal};
use crate::strategy::{GuessPool, Pool};

/// What one earlier guess demands of every later guess under the official hard-mode rule
/// (Selby's `okhard`): keep each green letter in its position, and use each green or yellow
/// letter at least as many times as it was scored green or yellow. Grays may be replayed, and a
/// yellow may repeat its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardRule<T> {
    /// Each green letter with its position, ascending by position.
    greens: Vec<(usize, T)>,
    /// Each green or yellow letter with the fewest times a word must hold it.
    needs: Vec<(T, usize)>,
}

/// How a word breaks a [`HardRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation<T> {
    /// A green letter must stay at this position.
    Position(usize, T),
    /// The letter must appear at least this many times.
    Letter(T, usize),
}

impl<T: Copy + PartialEq> HardRule<T> {
    /// The rule from a guess's `letters` and the base-3 `ordinal` of its feedback (position 0 is
    /// the most significant digit; 0 absent, 1 misplaced, 2 correct).
    pub(crate) fn new<const N: usize>(letters: &[T; N], mut ordinal: u64) -> Self {
        let mut greens = Vec::new();
        let mut needs: Vec<(T, usize)> = Vec::new();
        for position in (0..N).rev() {
            let digit = ordinal % 3;
            ordinal /= 3;
            if digit == 0 {
                continue;
            }
            let letter = letters[position];
            if digit == 2 {
                greens.push((position, letter));
            }
            match needs.iter_mut().find(|(l, _)| *l == letter) {
                Some((_, n)) => *n += 1,
                None => needs.push((letter, 1)),
            }
        }
        greens.reverse();
        Self { greens, needs }
    }

    /// The first way `word` breaks the rule, or `None` when it keeps it.
    pub(crate) fn violation<const N: usize>(&self, word: &[T; N]) -> Option<Violation<T>> {
        if let Some(&(position, letter)) = self.greens.iter().find(|&&(p, l)| word[p] != l) {
            return Some(Violation::Position(position, letter));
        }
        self.needs.iter().find_map(|&(letter, need)| {
            (word.iter().filter(|&&l| l == letter).count() < need)
                .then_some(Violation::Letter(letter, need))
        })
    }

    /// Whether `word` keeps the rule.
    pub(crate) fn allows<const N: usize>(&self, word: &[T; N]) -> bool {
        self.violation(word).is_none()
    }
}

impl fmt::Display for Violation<char> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Position(position, letter) => write!(
                f,
                "must keep {} in position {}",
                letter.to_uppercase(),
                position + 1
            ),
            Self::Letter(letter, 1) => write!(f, "must use {}", letter.to_uppercase()),
            Self::Letter(letter, n) => write!(f, "must use {} {n} times", letter.to_uppercase()),
        }
    }
}

/// The guesses a game position allows, as indices into the pool.
///
/// `Whole` and `Candidates` store nothing: the pool, or the node's own candidate set. `List`
/// is an ascending index list that always contains the candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Allowed {
    /// Every pool word.
    Whole,
    /// Exactly the candidates.
    Candidates,
    /// Ascending pool indices.
    List(Vec<u32>),
}

impl Allowed {
    /// The allowed guesses before any guess is played. Ultrahard on a Corpus run (every pool word
    /// is a candidate) is the candidate set at every depth, so it stores nothing.
    pub(crate) const fn root(mode: GuessPool, candidates_are_whole_pool: bool) -> Self {
        match mode {
            GuessPool::Ultrahard if candidates_are_whole_pool => Self::Candidates,
            GuessPool::All | GuessPool::Hard | GuessPool::Ultrahard => Self::Whole,
        }
    }

    /// The allowed guesses after `guess` scored `ordinal`.
    pub(crate) fn after<const N: usize>(
        &self,
        mode: GuessPool,
        pool: &Pool<N>,
        guess: &Code<N>,
        ordinal: u64,
    ) -> Self {
        let keep: Box<dyn Fn(u32) -> bool + '_> = match mode {
            GuessPool::All => return self.clone(),
            GuessPool::Hard => {
                let rule = HardRule::new(guess, ordinal);
                Box::new(move |i| rule.allows(pool.code(i)))
            }
            GuessPool::Ultrahard => {
                if *self == Self::Candidates {
                    return Self::Candidates;
                }
                Box::new(move |i| {
                    let mut counts: LetterCounts = [0; 256];
                    pattern_ordinal(pool.code(i), guess, &mut counts) == ordinal
                })
            }
        };
        match self {
            Self::Candidates => Self::Candidates,
            Self::Whole => Self::List((0..cast::narrow(pool.len())).filter(|&i| keep(i)).collect()),
            Self::List(list) => Self::List(list.iter().copied().filter(|&i| keep(i)).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters(word: &str) -> [char; 5] {
        word.chars().collect::<Vec<_>>().try_into().unwrap()
    }

    fn rule(guess: &str, feedback: &str) -> HardRule<char> {
        let ordinal = feedback
            .chars()
            .fold(0, |acc, c| acc * 3 + u64::from(c.to_digit(10).unwrap()));
        HardRule::new(&letters(guess), ordinal)
    }

    fn allows(rule: &HardRule<char>, word: &str) -> bool {
        rule.allows(&letters(word))
    }

    #[test]
    fn greens_stay_in_place() {
        let r = rule("crane", "20000");
        assert!(allows(&r, "cling"));
        assert!(!allows(&r, "stood"));
        assert_eq!(
            r.violation(&letters("stood")),
            Some(Violation::Position(0, 'c'))
        );
    }

    #[test]
    fn yellows_must_reappear_and_may_repeat_their_position() {
        let r = rule("crane", "01000");
        assert!(allows(&r, "arbor"), "R present");
        assert!(allows(&r, "mrown"), "R may sit where it was a yellow");
        assert!(!allows(&r, "stood"));
    }

    #[test]
    fn grays_may_be_replayed() {
        let r = rule("crane", "00000");
        assert!(allows(&r, "crane"));
    }

    #[test]
    fn multiplicity_counts_green_and_yellow_together() {
        // two E scored (one green, one yellow): the next guess needs two E
        let r = rule("geese", "22100");
        assert!(allows(&r, "geeky"));
        assert!(!allows(&r, "gaily"));
        assert_eq!(
            r.violation(&letters("gelid")),
            Some(Violation::Letter('e', 2))
        );
    }

    #[test]
    fn a_gray_duplicate_adds_no_requirement() {
        // one E scored yellow, the second gray: one E is enough
        let r = rule("geese", "00100");
        assert!(allows(&r, "bevel") && allows(&r, "creak"));
    }

    #[test]
    fn violations_read_as_one_line_reasons() {
        assert_eq!(
            Violation::Position(1, 'r').to_string(),
            "must keep R in position 2"
        );
        assert_eq!(Violation::Letter('e', 1).to_string(), "must use E");
        assert_eq!(Violation::Letter('e', 2).to_string(), "must use E 2 times");
    }
}
