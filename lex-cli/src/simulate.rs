// TODO: future frontend crates follow this pattern:
//   lex-tui/   — terminal UI (ratatui or similar), depends on lex-core + lex-data only
//   lex-cli/   — batch simulation CLI (clap), depends on lex-core + lex-data only
// A TUI is a separate crate rather than a feature flag here: it is an event loop with render
// state and would share lex-core but nothing from lex-cli.

use std::collections::HashMap;
use std::io::Write;

use lex_core::{CANONICAL_TEMPERATURE, Guesser, answer_prior, play};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::cli::SimulateArgs;
use crate::lists::{GameLists, Source};
use crate::report::{Game, Report, Run, SCHEMA_VERSION, Summary};
use crate::targets;

pub fn simulate<const N: usize>(args: &SimulateArgs) -> anyhow::Result<()> {
    let data_dir = lex_data::DataDir::new(&args.common.data_dir);
    let (lists, source) = Source::load::<N>(&args.common, &data_dir)?;

    // The evaluation prior is fixed at the canonical temperature whatever the solver uses, and is
    // taken over the whole pool before any game narrows the candidate set; only the Answer
    // list's words are ever looked up.
    let priors: HashMap<_, _> = answer_prior(&lists.pool, CANONICAL_TEMPERATURE)
        .into_iter()
        .collect();
    let targets = targets::select(
        lists.answers.clone(),
        &source.answer_noun(),
        args.targets.as_deref(),
        args.num_games,
        args.seed,
    )?;
    let num_games = targets.len();

    log::info!(
        "Simulating {num_games} games with {N}-letter words in {} ({} corpus words)...",
        source.name(),
        source.dictionary_length()
    );

    // One guesser for every game: they share its decision tree, so each position is scored once.
    let strategy = args.common.strategy();
    let GameLists { pool, answers } = lists;
    let mut guesser = Guesser::new(pool, answers, strategy);
    let first_guess = guesser
        .suggest()
        .ok_or_else(|| anyhow::anyhow!("no valid first guess"))?;

    let mut games = Vec::with_capacity(num_games);
    for word in targets {
        let result = play(word, &mut guesser, Vec::new())?;
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
        games.push(Game {
            word: word.to_string(),
            num_guesses: result.num_guesses(),
            prior: priors[&word],
            guesses: result
                .guesses()
                .iter()
                .map(|g| g.word().to_string())
                .collect(),
        });
    }

    log::info!("Completed {num_games} games");

    let report = Report {
        schema_version: SCHEMA_VERSION,
        run: Run {
            lex_version: env!("CARGO_PKG_VERSION").to_owned(),
            timestamp: OffsetDateTime::now_utc().format(&Rfc3339)?,
            seed: args.seed,
            num_games,
            targets_file: args.targets.as_ref().map(|p| p.display().to_string()),
            first_guess: first_guess.to_string(),
            strategy: strategy.into(),
        },
        corpus: source.corpus().clone(),
        profile: source.profile_report(),
        summary: Summary::from_games(&games),
        games,
    };

    match args.output.as_deref() {
        Some(path) if path.as_os_str() == "-" => {
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &report)?;
            writeln!(stdout)?;
            eprint!("{}", report.render());
        }
        Some(path) => {
            std::fs::write(path, serde_json::to_string_pretty(&report)? + "\n")?;
            print!("{}", report.render());
        }
        None => print!("{}", report.render()),
    }
    Ok(())
}
