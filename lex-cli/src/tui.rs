//! `lex tui`: the game setup the terminal UI plays over, as `lex-tui`'s [`Session`].
//!
//! Only this module is generic over the word length; `lex-tui` sees plain strings.

use std::sync::Arc;

use lex_core::{Correctness, Explanation, Guess, Guesser, WordCorrectness};
use lex_data::Word;
use lex_tui::{ExplanationView, GameInfo, Played, Row, Session};

use crate::books::{self, BookMode};
use crate::config::Targets;
use crate::lists::Source;
use crate::progress;
use crate::targets;

use crate::cli::TuiArgs;

/// A game over `N`-letter words, solved on the `--threads` pool.
struct WatchSession<const N: usize> {
    /// The solver, replayed to the position asked for.
    guesser: Guesser<N>,
    /// The Answer list, the words a game can be about.
    answers: Vec<Word<N>>,
    /// The Profile or language name.
    name: String,
    /// Names the Answer list in messages.
    noun: String,
    /// The pool every solver call runs inside, since the UI loop runs outside it.
    pool: Arc<rayon::ThreadPool>,
}

/// Loads the lists and the Opening book for `args`, building the book if it is missing, and
/// returns the session over them. Slow, so it runs before the terminal is taken over.
///
/// # Errors
///
/// If the lists or the Opening book cannot be loaded, or the game has no answers.
pub fn session<const N: usize>(
    args: &TuiArgs,
    pool: &Arc<rayon::ThreadPool>,
) -> anyhow::Result<Box<dyn Session + Send>> {
    let config = args.common.config(Targets::Full);
    let data_dir = args.common.data();
    pool.install(|| {
        let (lists, source) = Source::load::<N>(&config, &data_dir)?;
        anyhow::ensure!(!lists.answers.is_empty(), "the game has no answers");
        let answers = lists.answers.clone();
        let mut guesser = books::guesser(
            &config,
            &data_dir,
            &source,
            lists,
            BookMode::from_no_book(args.no_book),
            true,
            progress::observer().as_deref(),
        )?;
        guesser.set_explain(true);
        let session: Box<dyn Session + Send> = Box::new(WatchSession {
            guesser,
            answers,
            name: source.name().to_owned(),
            noun: source.answer_noun(),
            pool: Arc::clone(pool),
        });
        Ok(session)
    })
}

impl<const N: usize> WatchSession<N> {
    /// The word `text` spells, if it has `N` letters.
    fn word(text: &str) -> Result<Word<N>, String> {
        Word::<N>::try_from(text.trim().to_lowercase().as_str()).map_err(|e| e.to_string())
    }

    /// The guesser at the position after `history`.
    fn replay(&mut self, history: &[Played]) {
        self.guesser.reset();
        for played in history {
            let (Ok(word), true) = (Self::word(&played.word), played.feedback.len() == N) else {
                continue;
            };
            let mut feedback = WordCorrectness::<N>::absent();
            feedback.copy_from_slice(&played.feedback);
            self.guesser.push_guess(Guess::new(word, feedback));
        }
    }
}

impl<const N: usize> Session for WatchSession<N> {
    fn game(&self) -> GameInfo {
        GameInfo {
            name: self.name.clone(),
            word_length: N,
        }
    }

    fn draw(&self, seed: u64) -> String {
        // `session` rejects an empty Answer list, so a sample of one always exists.
        targets::select(self.answers.clone(), &self.noun, None, Some(1), seed)
            .ok()
            .and_then(|drawn| drawn.first().map(ToString::to_string))
            .unwrap_or_default()
    }

    fn check_target(&self, word: &str) -> Result<String, String> {
        let parsed = Self::word(word)?;
        if self.answers.contains(&parsed) {
            Ok(parsed.to_string())
        } else {
            Err(format!("{parsed} is not in {}", self.noun))
        }
    }

    fn play(&self, guess: &str, target: &str) -> Vec<Correctness> {
        match (Self::word(guess), Self::word(target)) {
            (Ok(guess), Ok(target)) => WordCorrectness::correct(target, guess).to_vec(),
            (Err(_), _) | (_, Err(_)) => vec![Correctness::Absent; N],
        }
    }

    fn explain(&mut self, history: &[Played]) -> Option<ExplanationView> {
        let pool = Arc::clone(&self.pool);
        pool.install(|| {
            self.replay(history);
            let candidates = self.guesser.num_candidates();
            let Some(Explanation {
                guess,
                technique,
                from_book,
                guesses,
                ..
            }) = self.guesser.explain()
            else {
                let guess = self.guesser.suggest()?;
                return Some(ExplanationView {
                    guess: guess.to_string(),
                    technique: None,
                    from_book: false,
                    candidates,
                    rows: Vec::new(),
                });
            };
            Some(ExplanationView {
                guess: guess.to_string(),
                technique: Some(technique.to_string()),
                from_book,
                candidates,
                rows: guesses
                    .iter()
                    .map(|s| Row {
                        word: s.word.to_string(),
                        bits: s.expected_bits,
                        remaining: s.expected_remaining,
                        cost: s.guess_cost,
                        in_candidates: s.in_candidates,
                    })
                    .collect(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use lex_data::testing::{seed_dict, seed_supported};
    use lex_data::{DataDir, Language};

    use super::*;

    /// Parses the flags of `lex tui` on their own.
    #[derive(clap::Parser)]
    struct Flags {
        #[command(flatten)]
        args: TuiArgs,
    }

    const G: Correctness = Correctness::Correct;
    const Y: Correctness = Correctness::Misplaced;
    const X: Correctness = Correctness::Absent;

    /// A session over a seeded English five-letter corpus, played without a book.
    fn seeded() -> (tempfile::TempDir, Box<dyn Session + Send>) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        seed_supported(
            &dir,
            Language::English,
            5,
            &[
                ("crane", 900),
                ("stare", 800),
                ("light", 700),
                ("mount", 600),
                ("plumb", 500),
                ("sound", 400),
            ],
        );
        seed_dict(&dir, Language::English);
        let flags = Flags::try_parse_from([
            "tui",
            "--lang",
            "english",
            "-l",
            "5",
            "--no-book",
            "--data-dir",
            tmp.path().to_str().unwrap(),
        ])
        .unwrap();
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let session = session::<5>(&flags.args, &pool).unwrap();
        (tmp, session)
    }

    #[test]
    fn draws_the_same_answer_for_the_same_seed() {
        let (_tmp, session) = seeded();
        assert_eq!(session.draw(7), session.draw(7));
        assert_eq!(session.check_target(&session.draw(7)), Ok(session.draw(7)));
    }

    #[test]
    fn a_target_must_be_an_answer_of_the_right_length() {
        let (_tmp, session) = seeded();
        assert_eq!(session.check_target(" Crane "), Ok("crane".to_owned()));
        assert!(
            session
                .check_target("bread")
                .unwrap_err()
                .contains("not in the dictionary")
        );
        assert!(session.check_target("cran").is_err());
    }

    #[test]
    fn play_gives_the_feedback_against_the_target() {
        let (_tmp, session) = seeded();
        assert_eq!(session.play("stare", "crane"), [X, X, G, Y, G]);
        assert_eq!(session.play("crane", "crane"), [G; 5]);
    }

    #[test]
    fn explains_the_opening_and_narrows_after_a_guess() {
        let (_tmp, mut session) = seeded();
        let opening = session.explain(&[]).unwrap();
        assert_eq!(
            opening.rows.first().map(|r| r.word.clone()),
            Some(opening.guess.clone())
        );
        assert!(opening.technique.is_some());
        let played = Played {
            word: "crane".to_owned(),
            feedback: session.play("crane", "light"),
        };
        let next = session.explain(&[played]).unwrap();
        assert!(next.candidates < opening.candidates);
        assert_eq!(session.game().word_length, 5);
    }
}
