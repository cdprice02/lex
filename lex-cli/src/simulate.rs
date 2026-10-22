// TODO: future frontend crates follow this pattern:
//   lex-tui/   — terminal UI (ratatui or similar), depends on lex-core + lex-data only
//   lex-cli/   — batch simulation CLI (clap), depends on lex-core + lex-data only
// A TUI is a separate crate rather than a feature flag here: it is an event loop with render
// state and would share lex-core but nothing from lex-cli.

use std::collections::HashMap;
use std::io::Write;

use lex_core::{Guess, Guesser, WordCorrectness, play};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::cli::SimulateArgs;
use crate::report::{Corpus, DictProvenance, Game, Report, Run, SCHEMA_VERSION, Summary};
use crate::targets;

pub fn simulate<const N: usize>(args: &SimulateArgs) -> anyhow::Result<()> {
    let data_dir = lex_data::blocking::DataDir::new(&args.common.data_dir);
    let lang = args.common.lang;
    let word_set = data_dir.load::<N>(lang, args.common.dictionary_length)?;
    let dictionary_length = word_set.len();

    // Priors are taken over the full dictionary, before any game narrows the candidate set.
    let priors: HashMap<_, _> = word_set.word_probs().collect();
    let targets = targets::select(
        &word_set,
        args.targets.as_deref(),
        args.num_games,
        args.seed,
    )?;
    let num_games = targets.len();

    log::info!("Simulating {num_games} games with {N}-letter words in {lang}...");

    //* IMPORTANT: this relies on Guesser being deterministic — the same word_set always
    //* produces the same first suggestion. If Guesser ever introduces randomness or
    //* word-dependent state, this cache must be removed.
    let first_guess = Guesser::new(word_set.clone())
        .suggest()
        .ok_or_else(|| anyhow::anyhow!("no valid first guess"))?;

    let mut games = Vec::with_capacity(num_games);
    for word in targets {
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

    let dict = match data_dir.dict_metadata(lang) {
        Ok(meta) => Some(DictProvenance {
            source_url: meta.source_url().map(str::to_owned),
            fetched_at: meta.fetched_at().map(str::to_owned),
            source_sha256: meta.source_sha256().map(str::to_owned),
            word_count: meta.word_count,
        }),
        Err(e) => {
            log::warn!("no dictionary metadata for the report: {e:#}");
            None
        }
    };
    let report = Report {
        schema_version: SCHEMA_VERSION,
        run: Run {
            lex_version: env!("CARGO_PKG_VERSION").to_owned(),
            timestamp: OffsetDateTime::now_utc().format(&Rfc3339)?,
            seed: args.seed,
            num_games,
            targets_file: args.targets.as_ref().map(|p| p.display().to_string()),
            first_guess: first_guess.to_string(),
        },
        corpus: Corpus {
            language: lang.to_string(),
            word_length: N,
            dictionary_length,
            ngrams_snapshot: lex_data::NGRAMS_SNAPSHOT.to_owned(),
            ngrams_sha256: data_dir.ngrams_sha256(lang, N)?,
            dict,
        },
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
