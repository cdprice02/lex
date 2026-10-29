//! Opening book files for a run: key the book, load it, or build and store it on first use
//! (ADR 0011). `lex-data` owns the file; `lex-core` converts and validates the nodes.

use std::time::{Duration, Instant};

use anyhow::Context;
use lex_core::{Guesser, OpeningBook, SOLVER_DIGEST};
use lex_data::{BookKey, BookLookup, DataDir, ProfileKey};

use crate::cli::BookBuildArgs;
use crate::config::{Game, RunConfig};
use crate::lists::{GameLists, Source};

/// Whether a run plays with the Opening book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookMode {
    /// Load the book, building it first when it is missing or stale.
    Auto,
    /// Play fully online (`--no-book`).
    Off,
}

impl BookMode {
    /// `Off` when `--no-book` was given, else `Auto`.
    pub fn from_no_book(no_book: bool) -> Self {
        if no_book { Self::Off } else { Self::Auto }
    }
}

/// How a book came to be ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Loaded from its file.
    UpToDate,
    /// Built in this long, then stored.
    Built(Duration),
}

/// A book ready for play and how it got there.
pub struct Ensured<const N: usize> {
    /// The book.
    pub book: OpeningBook<N>,
    /// Whether the book was loaded or built.
    pub status: Status,
    /// The key fields on which the closest book on disk differed, when this book was built
    /// because that one was stale.
    pub stale_fields: Option<Vec<String>>,
    /// The book file.
    pub path: std::path::PathBuf,
    /// Size of the book file in bytes.
    pub bytes: u64,
}

/// The key of the book for `config` over `lists`: everything that can change a chosen guess.
pub fn key<const N: usize>(config: &RunConfig, source: &Source, lists: &GameLists<N>) -> BookKey {
    let profile = match source {
        Source::Corpus(_) => None,
        Source::Profile { stats, .. } => Some(ProfileKey {
            name: stats.name.clone(),
            answers_sha256: stats.answers_sha256.clone(),
            guess_only_sha256: stats.guess_only_sha256.clone(),
        }),
    };
    BookKey {
        corpus_sha256: source.corpus().ngrams_sha256.clone(),
        profile,
        word_length: N,
        dictionary_length: lists.pool.len(),
        strategy: config
            .strategy
            .key_fields()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        solver_digest: SOLVER_DIGEST.to_owned(),
    }
}

/// The language of `game`'s Corpus.
fn language(game: &Game) -> lex_data::Language {
    match game {
        Game::Corpus { language, .. } => *language,
        Game::Profile(profile) => profile.spec().language,
    }
}

/// Loads the book for `config`, building and storing it first when no file matches the key or
/// `force` asks for a rebuild. A stale book is rebuilt with a message naming what changed; a
/// damaged one is an error.
pub fn ensure<const N: usize>(
    config: &RunConfig,
    data_dir: &DataDir,
    source: &Source,
    lists: &GameLists<N>,
    force: bool,
) -> anyhow::Result<Ensured<N>> {
    let lang = language(&config.game);
    let key = key(config, source, lists);
    let path = data_dir.book_path(lang, &key);
    let mut stale_fields = None;
    if !force {
        match data_dir.read_book(lang, &key)? {
            BookLookup::Found(stored) => {
                let bytes = std::fs::metadata(&path)?.len();
                let book = OpeningBook::<N>::from_nodes(stored, lists.pool.len()).with_context(
                    || format!("opening book {path:?} is corrupt; rebuild it with `lex book build --force`"),
                )?;
                return Ok(Ensured {
                    book,
                    status: Status::UpToDate,
                    stale_fields: None,
                    path,
                    bytes,
                });
            }
            BookLookup::Missing { nearest: None } => {
                eprintln!("no opening book for {}; building it", source.name());
            }
            BookLookup::Missing {
                nearest: Some(fields),
            } => {
                eprintln!(
                    "no opening book for this key (the closest on disk differs in {}); building it",
                    fields.join(", ")
                );
                stale_fields = Some(fields);
            }
        }
    }

    let started = Instant::now();
    let guesser = Guesser::new(lists.pool.clone(), lists.answers.clone(), config.strategy);
    let mut last_report = Instant::now();
    let book = guesser.build_book_reporting(&mut |booked| {
        if last_report.elapsed() >= Duration::from_secs(5) {
            eprintln!(
                "  booked {booked} positions ({:?} elapsed)",
                started.elapsed()
            );
            last_report = Instant::now();
        }
    });
    let elapsed = started.elapsed();
    let file = data_dir.write_book(lang, &key, &book.to_nodes())?;
    eprintln!(
        "built the opening book: {} positions, {} bytes, {elapsed:.1?}",
        book.len(),
        file.bytes
    );
    Ok(Ensured {
        book,
        status: Status::Built(elapsed),
        stale_fields,
        path: file.path,
        bytes: file.bytes,
    })
}

/// A guesser for `lists`, seeded with the Opening book unless `mode` is off.
pub fn guesser<const N: usize>(
    config: &RunConfig,
    data_dir: &DataDir,
    source: &Source,
    lists: GameLists<N>,
    mode: BookMode,
) -> anyhow::Result<Guesser<N>> {
    let strategy = config.strategy;
    if mode == BookMode::Off {
        let GameLists { pool, answers } = lists;
        return Ok(Guesser::new(pool, answers, strategy));
    }
    let ensured = ensure(config, data_dir, source, &lists, false)?;
    let GameLists { pool, answers } = lists;
    Ok(Guesser::with_book(pool, answers, strategy, ensured.book))
}

/// The `lex book build` command: builds the book, or reports that it is up to date.
pub fn build_command<const N: usize>(args: &BookBuildArgs) -> anyhow::Result<()> {
    let config = args.config();
    let data_dir = DataDir::new(&args.common.data_dir);
    let (lists, source) = Source::load::<N>(&config, &data_dir)?;
    let ensured = ensure(&config, &data_dir, &source, &lists, args.force)?;
    let state = match ensured.status {
        Status::UpToDate => "up to date".to_owned(),
        Status::Built(elapsed) => format!("built in {elapsed:.1?}"),
    };
    println!(
        "opening book {state}: {} positions, {} bytes, {}",
        ensured.book.len(),
        ensured.bytes,
        ensured.path.display()
    );
    Ok(())
}
