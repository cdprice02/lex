//! `lex simulate`: plays a batch of games and reports the results.

// TODO: future frontend crates follow this pattern:
//   lex-tui/   — terminal UI (ratatui or similar), depends on lex-core + lex-data only
//   lex-cli/   — batch simulation CLI (clap), depends on lex-core + lex-data only
// A TUI is a separate crate rather than a feature flag here: it is an event loop with render
// state and would share lex-core but nothing from lex-cli.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use lex_core::{CANONICAL_TEMPERATURE, answer_prior, play};
use lex_data::DataDir;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::books::{self, BookMode};
use crate::cli::SimulateArgs;
use crate::config::{RunConfig, Targets};
use crate::lists::Source;
use crate::progress::{self, Observer};
use crate::report::{Game, Report, Run, SCHEMA_VERSION, Summary};
use crate::targets;

/// The `simulate` command: plays what the arguments ask for, then prints or writes the report.
///
/// # Errors
///
/// If the run fails (see [`run`]) or the report cannot be written.
pub fn simulate<const N: usize>(args: &SimulateArgs) -> anyhow::Result<()> {
    let mode = BookMode::from_no_book(args.no_book);
    let observer = progress::observer();
    let report = run_n::<N>(&(
        &args.config(),
        &args.common.data(),
        mode,
        observer.as_deref(),
    ))?;
    // Drop the bars before the summary prints under them.
    drop(observer);
    emit(&report, args.output.as_deref())
}

/// Plays `config` with its Opening book, built on first use, and returns the report. `observer`
/// hears the book build and every game; `None` draws and prints nothing.
///
/// # Errors
///
/// If the lists cannot be loaded, the book cannot be built or read, or a game finds no valid word.
pub fn run(
    config: &RunConfig,
    data_dir: &DataDir,
    observer: Option<&dyn Observer>,
) -> anyhow::Result<Report> {
    run_with(config, data_dir, BookMode::Auto, observer)
}

/// As [`run`], with the Opening book used or not as `mode` says.
///
/// # Errors
///
/// As [`run`].
pub fn run_with(
    config: &RunConfig,
    data_dir: &DataDir,
    mode: BookMode,
    observer: Option<&dyn Observer>,
) -> anyhow::Result<Report> {
    lex_data::match_word_length!(
        run_n,
        config.game.word_length(),
        &(config, data_dir, mode, observer)
    )
}

/// Plays every target of `config` and builds its report.
fn run_n<const N: usize>(
    (config, data_dir, mode, observer): &(&RunConfig, &DataDir, BookMode, Option<&dyn Observer>),
) -> anyhow::Result<Report> {
    let (lists, source) = Source::load::<N>(config, data_dir)?;

    // The evaluation prior is fixed at the canonical temperature whatever the solver uses, and is
    // taken over the whole pool before any game narrows the candidate set; only the Answer
    // list's words are ever looked up.
    let priors: HashMap<_, _> = answer_prior(&lists.pool, CANONICAL_TEMPERATURE)
        .into_iter()
        .collect();
    let (file, num_games, seed) = match &config.targets {
        Targets::Full => (None, None, 0),
        Targets::Sample { n, seed } => (None, Some(*n), *seed),
        Targets::File(path) => (Some(path.as_path()), None, 0),
    };
    let targets = targets::select(
        lists.answers.clone(),
        &source.answer_noun(),
        file,
        num_games,
        seed,
    )?;
    let num_games = targets.len();

    log::info!(
        "Simulating {num_games} games with {N}-letter words in {} ({} corpus words)...",
        source.name(),
        source.dictionary_length()
    );

    // One guesser for every game: they share its decision tree, so each position is scored once.
    let mut guesser = books::guesser(config, data_dir, &source, lists, *mode, *observer)?;
    let first_guess = guesser
        .suggest()
        .ok_or_else(|| anyhow::anyhow!("no valid first guess"))?;

    if let Some(observer) = observer {
        observer.games_started(num_games);
    }
    let mut games = Vec::with_capacity(num_games);
    for word in targets {
        let result = play(word, &mut guesser, Vec::new())?;
        let label = match result.num_guesses() {
            1 => "Genius",
            2 => "Magnificent",
            3 => "Impressive",
            4 => "Splendid",
            5 => "Great",
            _ => "Phew",
        };
        log::info!("{label}");
        if let Some(observer) = observer {
            observer.game_played();
        }
        games.push(Game {
            word: word.to_string(),
            num_guesses: u32::try_from(result.num_guesses())?,
            prior: priors[&word],
            guesses: result
                .guesses()
                .iter()
                .map(|g| g.word().to_string())
                .collect(),
        });
    }

    log::info!("Completed {num_games} games");

    assemble(config, &source, file, seed, first_guess.to_string(), games)
}

/// The report of `games`, which `config` and `source` produced. Not generic over the word length,
/// so it is compiled once.
fn assemble(
    config: &RunConfig,
    source: &Source,
    targets_file: Option<&Path>,
    seed: u64,
    first_guess: String,
    games: Vec<Game>,
) -> anyhow::Result<Report> {
    Ok(Report {
        schema_version: SCHEMA_VERSION,
        run: Run {
            lex_version: env!("CARGO_PKG_VERSION").to_owned(),
            timestamp: OffsetDateTime::now_utc().format(&Rfc3339)?,
            seed,
            num_games: games.len(),
            targets_file: targets_file.map(|p| p.display().to_string()),
            first_guess,
            strategy: config.strategy.into(),
        },
        corpus: source.corpus().clone(),
        profile: source.profile_report(),
        summary: Summary::from_games(&games),
        games,
    })
}

/// Writes the full report to `output` (`-` for stdout, with the summary on stderr), else prints the summary.
fn emit(report: &Report, output: Option<&Path>) -> anyhow::Result<()> {
    match output {
        Some(path) if path.as_os_str() == "-" => {
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &report)?;
            writeln!(stdout)?;
            anstream::eprint!("{}", report.render_styled(true));
        }
        Some(path) => {
            std::fs::write(path, serde_json::to_string_pretty(&report)? + "\n")?;
            anstream::print!("{}", report.render_styled(true));
        }
        None => anstream::print!("{}", report.render_styled(true)),
    }
    Ok(())
}
