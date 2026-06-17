// TODO: future frontend crates follow this pattern:
//   lex-tui/   — terminal UI (ratatui or similar), depends on lex-core + lex-data only
//   lex-cli/   — batch simulation CLI (clap), depends on lex-core + lex-data only
// A TUI is a separate crate rather than a feature flag here: it is an event loop with render
// state and would share lex-core but nothing from lex-cli.

use lex_core::{Guess, Guesser, WordCorrectness, play};

use crate::cli::Args;

// Generates the match_word_length_run! dispatch macro.
// seq_macro requires literal bounds, so $min/$max must be kept in sync with
// lex_data::MIN_WORD_LENGTH and lex_data::MAX_WORD_LENGTH — enforced at compile time below.
macro_rules! configure_word_length_bounds {
    ($min:literal, $max:literal) => {
        const _: () = assert!(
            $min == lex_data::MIN_WORD_LENGTH && $max == lex_data::MAX_WORD_LENGTH,
            "configure_word_length_bounds! literals must match lex_data word length bounds"
        );

        macro_rules! match_word_length_run {
                                            ($args:expr) => {
                                                seq_macro::seq!(N in $min..=$max {
                                                    match ($args).word_length {
                                                        #(N => simulate::simulate::<N>($args),)*
                                                        _ => unreachable!(),
                                                    }
                                                })
                                            };
                                        }
    };
}

pub fn simulate<const N: usize>(args: &Args) -> anyhow::Result<()> {
    let word_set = lex_data::blocking::DataDir::new(&args.data_dir)
        .load::<N>(args.lang, args.dictionary_length)?;
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
        let label = match result.num_guesses() {
            0 => unreachable!("game must have at least one guess to complete"),
            1 => "Genius",
            2 => "Magnificent",
            3 => "Impressive",
            4 => "Splendid",
            5 => "Great",
            _ => "Phew",
        };
        log::info!("{}", label);
        results.push(result);
    }

    log::info!("Completed {} games", num_games);

    let avg_guesses =
        results.iter().map(|r| r.num_guesses() as f64).sum::<f64>() / num_games as f64;
    log::info!("Average number of guesses: {avg_guesses:.2}");

    Ok(())
}
