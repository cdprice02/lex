// TODO: future frontend crates follow this pattern:
//   lex-tui/   — terminal UI (ratatui or similar), depends on lex-core + lex-data only
//   lex-cli/   — batch simulation CLI (clap), depends on lex-core + lex-data only
// A TUI is a separate crate rather than a feature flag here: it is an event loop with render
// state and would share lex-core but nothing from lex-cli.

use lex_core::{Guess, Guesser, WordCorrectness, play};

use crate::cli::SimulateArgs;

pub fn simulate<const N: usize>(args: &SimulateArgs) -> anyhow::Result<()> {
    let word_set = lex_data::blocking::DataDir::new(&args.common.data_dir)
        .load::<N>(args.common.lang, args.common.dictionary_length)?;
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
        args.common.lang
    );

    //* IMPORTANT: this relies on Guesser being deterministic — the same word_set always
    //* produces the same first suggestion. If Guesser ever introduces randomness or
    //* word-dependent state, this cache must be removed.
    let first_guess = Guesser::new(word_set.clone())
        .suggest()
        .ok_or_else(|| anyhow::anyhow!("no valid first guess"))?;

    let mut results = Vec::new();
    for word in word_set.words().cycle().take(num_games) {
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
