//! `Guesser::explain` reports the guess `suggest` plays, at every position of the first games of
//! three Tracks. Ignored by default: it reads the local Corpus data and builds the Opening books
//! the Tracks use (`LEX_DATA_DIR` or the workspace's `data/`). Run with `-- --ignored`.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::unwrap_used,
    reason = "an integration test helper aborts on a failed fixture"
)]

use std::path::PathBuf;

use lex_cli::books::{self, BookMode};
use lex_cli::lists::Source;
use lex_cli::tracks::Track;
use lex_core::{Guess, WordCorrectness};
use lex_data::DataDir;

/// Games played per Track.
const GAMES: usize = 20;

fn data_dir() -> PathBuf {
    std::env::var_os("LEX_DATA_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data"),
        PathBuf::from,
    )
}

fn agrees(id: &str) {
    let track = Track::find(id).unwrap();
    let config = track.config();
    let data = DataDir::new(data_dir());
    let (lists, source) = Source::load::<5>(&config, &data).unwrap();
    let targets: Vec<_> = lists.answers.iter().copied().take(GAMES).collect();
    let (again, _) = Source::load::<5>(&config, &data).unwrap();
    // two guessers on the same book: one records, one is the plain `suggest` the Track plays
    let build = |lists| books::guesser(&config, &data, &source, lists, BookMode::Auto, None);
    let mut explained = build(lists).unwrap();
    let mut plain = build(again).unwrap();
    explained.set_explain(true);
    let mut positions = 0;
    for target in targets {
        explained.reset();
        plain.reset();
        loop {
            let explanation = explained.explain().unwrap();
            let suggested = plain.suggest().unwrap();
            assert_eq!(explanation.guess, suggested, "{id}: {target}");
            assert_eq!(explained.suggest(), Some(suggested), "{id}: {target}");
            positions += 1;
            let guess = Guess::new(suggested, WordCorrectness::correct(target, suggested));
            if guess.correctness().is_correct() {
                break;
            }
            explained.push_guess(guess.clone());
            plain.push_guess(guess);
        }
    }
    assert!(positions >= GAMES, "{id}: {positions} positions");
}

#[test]
#[ignore = "reads the Corpus data and builds Opening books"]
fn english_5() {
    agrees("english-5");
}

#[test]
#[ignore = "reads the Corpus data and builds Opening books"]
fn russian_5() {
    agrees("russian-5");
}

#[test]
#[ignore = "reads the Corpus data and builds Opening books"]
fn wordle_hard() {
    agrees("wordle-hard");
}
