//! A scripted [`Session`] for tests: no solver, no thread.

use lex_core::Correctness;

use crate::session::{ExplanationView, GameInfo, Played, Row, Session, SessionError};

/// The answers the fake draws from, by seed modulo their count.
pub const ANSWERS: [&str; 3] = ["light", "crane", "mount"];

/// The guesses the fake "solver" plays, by how many guesses came before.
const SCRIPT: [&str; 4] = ["slate", "moist", "mount", "light"];

/// A five-letter game whose solver follows [`SCRIPT`].
pub struct Fake;

impl Session for Fake {
    fn game(&self) -> GameInfo {
        GameInfo {
            name: "wordle".to_owned(),
            word_length: 5,
        }
    }

    fn draw(&self, seed: u64) -> Result<String, SessionError> {
        let index = usize::try_from(seed % 3).unwrap_or_default();
        Ok(ANSWERS[index].to_owned())
    }

    fn check_target(&self, word: &str) -> Result<String, SessionError> {
        let word = word.trim().to_lowercase();
        if ANSWERS.contains(&word.as_str()) {
            Ok(word)
        } else {
            Err(SessionError::new(format!(
                "{word} is not in the wordle answer list"
            )))
        }
    }

    fn play(&self, guess: &str, target: &str) -> Result<Vec<Correctness>, SessionError> {
        let target: Vec<char> = target.chars().collect();
        Ok(guess
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if target.get(i) == Some(&c) {
                    Correctness::Correct
                } else if target.contains(&c) {
                    Correctness::Misplaced
                } else {
                    Correctness::Absent
                }
            })
            .collect())
    }

    fn explain(&mut self, history: &[Played]) -> Result<Option<ExplanationView>, SessionError> {
        let step = history.len();
        // The last scripted guess is the target of seed 0, so the script ends the game.
        let guess = SCRIPT[step.min(SCRIPT.len() - 1)];
        let rows = [guess, "crane", "irate", "stole", "adieu", "roate"]
            .into_iter()
            .zip([1.0, 1.4, 1.9, 2.6, 3.1, 4.0])
            .enumerate()
            .map(|(i, (word, remaining))| {
                let rank = f64::from(u8::try_from(i).unwrap_or_default());
                Row {
                    word: word.to_owned(),
                    bits: 0.3f64.mul_add(-rank, 5.0),
                    remaining: remaining * 100.0 / f64::from(u32::try_from(step + 1).unwrap_or(1)),
                    cost: (step > 0).then_some(0.1f64.mul_add(rank, 3.25)),
                    in_candidates: i % 2 == 0,
                }
            })
            .collect();
        Ok(Some(ExplanationView {
            guess: guess.to_owned(),
            technique: Some(if step == 0 { "Entropy" } else { "Rollout" }.to_owned()),
            from_book: step == 0,
            candidates: 2315 / (step + 1).pow(3),
            rows,
        }))
    }
}

/// The [`Session`] calls a [`Broken`] session can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// [`Session::draw`]
    Draw,
    /// [`Session::check_target`]
    CheckTarget,
    /// [`Session::play`]
    Play,
    /// [`Session::explain`]
    Explain,
}

/// A [`Fake`] whose `0` call always fails, with this message.
pub struct Broken(pub Call);

/// What a [`Broken`] session says when its call fails.
pub const BROKEN: &str = "the session is broken";

impl Session for Broken {
    fn game(&self) -> GameInfo {
        Fake.game()
    }

    fn draw(&self, seed: u64) -> Result<String, SessionError> {
        if self.0 == Call::Draw {
            return Err(SessionError::new(BROKEN));
        }
        Fake.draw(seed)
    }

    fn check_target(&self, word: &str) -> Result<String, SessionError> {
        if self.0 == Call::CheckTarget {
            return Err(SessionError::new(BROKEN));
        }
        Fake.check_target(word)
    }

    fn play(&self, guess: &str, target: &str) -> Result<Vec<Correctness>, SessionError> {
        if self.0 == Call::Play {
            return Err(SessionError::new(BROKEN));
        }
        Fake.play(guess, target)
    }

    fn explain(&mut self, history: &[Played]) -> Result<Option<ExplanationView>, SessionError> {
        if self.0 == Call::Explain {
            return Err(SessionError::new(BROKEN));
        }
        Fake.explain(history)
    }
}
