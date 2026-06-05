#![cfg_attr(test, feature(test))]
#![feature(optimize_attribute)]
#![feature(variant_count)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![warn(non_exhaustive_omitted_patterns)]

pub mod cli;
pub mod error;
pub mod game;
pub mod guesser;

use cli::Args;
use game::play;
use guesser::correctness::WordCorrectness;
use guesser::{Guess, Guesser};

macro_rules! configure_word_length_bounds {
    ($min:literal, $max:literal) => {
        pub const MIN_WORD_LENGTH: usize = $min;
        pub const MAX_WORD_LENGTH: usize = $max;

        #[macro_export]
        macro_rules! match_word_length_run {
            ($args:expr) => {
                seq_macro::seq!(N in $min..=$max {
                    match ($args).word_length {
                        #(
                            N => $crate::run::<N>($args),
                        )*
                        _ => unreachable!("parser enforces {}..={}", $crate::MIN_WORD_LENGTH, $crate::MAX_WORD_LENGTH),
                    }
                })
            };
        }
    };
}

configure_word_length_bounds!(3, 10);

pub fn run<const N: usize>(args: &Args) -> anyhow::Result<()> {
    let word_set = lex_data::blocking::get::<N>(&args.data_dir, args.lang, args.dictionary_length)?;
    let num_games = args.num_games.unwrap_or(word_set.len());
    let dictionary_length = word_set.len();
    if dictionary_length < num_games {
        log::warn!(
            "dictionary length ({dictionary_length}) is less than number of games ({num_games}), so some words may be repeated in the games"
        );
    }

    log::info!(
        "Simulating {} games with {}-letter words in {}...",
        num_games,
        N,
        args.lang
    );

    //* IMPORTANT: this relies on Guesser being deterministic — the same word_set always
    //* produces the same first suggestion. If Guesser ever introduces randomness or
    //* word-dependent state, this cache must be removed.
    let first_guess = Guesser::new(word_set.clone())
        .suggest()
        .ok_or_else(|| anyhow::anyhow!("no valid first guess"))?;

    let mut results = Vec::new();
    for &word in word_set.words().iter().cycle().take(num_games) {
        let history = vec![Guess::new(
            first_guess,
            WordCorrectness::correct(word, first_guess),
        )];
        let result = play(word, &word_set, history)?;
        log::debug!("{}: {}", result.word(), result.num_guesses());
        results.push(result);
    }

    log::info!("Completed {} games", num_games);

    let avg_guesses =
        results.iter().map(|r| r.num_guesses() as f64).sum::<f64>() / num_games as f64;
    log::info!("Average number of guesses: {avg_guesses:.2}");

    Ok(())
}
