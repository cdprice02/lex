#![cfg_attr(test, feature(test))]
#![feature(optimize_attribute)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![warn(non_exhaustive_omitted_patterns)]

pub mod cli;
pub mod error;
pub mod game;
pub mod guesser;

use cli::Args;
use game::play;

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
    // TODO: add word selection strategies (e.g. random, most/least frequent, etc.) instead of just taking the first n words
    let words = lex_data::blocking::get::<N>(&args.data_dir, args.lang, args.num_games)?.words();
    let num_words = words.len();

    log::info!(
        "Simulating {} games with {}-letter words in {}...",
        num_words,
        N,
        args.lang
    );

    let mut results = Vec::new();
    for word in words {
        let result = play(word, &args.data_dir, args.lang, args.dictionary_length)?;
        log::debug!("{}: {}", result.word(), result.num_guesses());
        results.push(result);
    }

    log::info!("Completed {} games", num_words);

    let avg_guesses =
        results.iter().map(|r| r.num_guesses() as f64).sum::<f64>() / num_words as f64;
    log::info!("Average number of guesses: {avg_guesses:.2}");

    Ok(())
}
