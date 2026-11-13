//! Watch end to end, headless: the real `Session` over a small seeded corpus, played by
//! `lex_tui::drive` with no terminal and no worker thread.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "an integration test helper aborts on a failed fixture"
)]

use std::sync::Arc;

use lex_cli::cli::{Cli, Command};
use lex_cli::tui::{Setup, session};
use lex_data::testing::{seed_dict, seed_supported};
use lex_data::{DataDir, Language};
use lex_tui::{Model, Msg, Options, Phase, Symbols, drive, enter, press};

/// The words of the seeded corpus, which are also its guesses and answers' head.
const WORDS: [&str; 6] = ["crane", "stare", "light", "mount", "plumb", "sound"];

/// Seeds an English five-letter corpus, opens the real session on it and plays `inputs`.
fn watch(seed: u64, inputs: impl IntoIterator<Item = Msg>) -> Model {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    let head: Vec<(&str, u64)> = WORDS
        .into_iter()
        .zip((1..=6).rev().map(|i| i * 100))
        .collect();
    seed_supported(&dir, Language::English, 5, &head);
    seed_dict(&dir, Language::English);
    let cli = Cli::parse_args([
        "lex",
        "tui",
        "--lang",
        "english",
        "-l",
        "5",
        "--data-dir",
        tmp.path().to_str().unwrap(),
    ])
    .unwrap();
    let Command::Tui(args) = cli.command else {
        panic!("`lex tui` parsed to another command");
    };
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap(),
    );
    let mut session = session::<5>(&Setup {
        args: &args,
        pool: &pool,
    })
    .unwrap();
    drive(
        &mut *session,
        Options {
            seed,
            color: false,
            symbols: Symbols::Letters,
        },
        inputs,
    )
}

/// Enough presses of `space` to finish any game on the seeded corpus.
fn spaces() -> impl Iterator<Item = Msg> {
    std::iter::repeat_n(press(' '), 40)
}

#[test]
fn a_seeded_game_ends_on_its_target() {
    let model = watch(3, spaces());
    assert_eq!(model.phase, Phase::Solved);
    let target = model.target.clone().unwrap();
    assert_eq!(model.history.last().map(|p| p.word.clone()), Some(target));
    assert!(model.history.last().unwrap().is_correct());
}

#[test]
fn a_typed_game_ends_on_the_typed_word() {
    let typed = "/mount".chars().map(press).chain([enter()]);
    let model = watch(3, typed.chain(spaces()));
    assert_eq!(model.phase, Phase::Solved);
    assert_eq!(model.target.as_deref(), Some("mount"));
    assert_eq!(model.history.last().map(|p| p.word.as_str()), Some("mount"));
}
