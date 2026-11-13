//! A scripted [`Session`] for tests: no solver, no thread.

use lex_core::Correctness;

use crate::session::{ExplanationView, GameInfo, Played, Row, Session};

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

    fn draw(&self, seed: u64) -> String {
        let index = usize::try_from(seed % 3).unwrap_or_default();
        ANSWERS[index].to_owned()
    }

    fn check_target(&self, word: &str) -> Result<String, String> {
        let word = word.trim().to_lowercase();
        if ANSWERS.contains(&word.as_str()) {
            Ok(word)
        } else {
            Err(format!("{word} is not in the wordle answer list"))
        }
    }

    fn play(&self, guess: &str, target: &str) -> Vec<Correctness> {
        let target: Vec<char> = target.chars().collect();
        guess
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
            .collect()
    }

    fn explain(&mut self, history: &[Played]) -> Option<ExplanationView> {
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
        Some(ExplanationView {
            guess: guess.to_owned(),
            technique: Some(if step == 0 { "Entropy" } else { "Rollout" }.to_owned()),
            from_book: step == 0,
            candidates: 2315 / (step + 1).pow(3),
            rows,
        })
    }
}
