//! Opening book files for a run: key the book, load it, or build and store it on first use
//! (ADR 0011). `lex-data` owns the file; `lex-core` converts and validates the nodes.

use std::time::{Duration, Instant};

use anyhow::Context;
use lex_core::{Guesser, OpeningBook, SOLVER_DIGEST};
use lex_data::{BookKey, BookLookup, DataDir, ProfileKey};

use crate::cli::BookBuildArgs;
use crate::config::{Game, RunConfig};
use crate::lists::{GameLists, Source};
use crate::progress::{self, Observer};

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
    #[must_use]
    pub const fn from_no_book(no_book: bool) -> Self {
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
#[must_use]
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
#[must_use]
pub const fn language(game: &Game) -> lex_data::Language {
    match game {
        Game::Corpus { language, .. } => *language,
        Game::Profile(profile) => profile.spec().language,
    }
}

/// Loads the book for `config`, building it first when none matches or `force` asks.
///
/// A missing book is built and stored. A stale book is rebuilt, the observer told what changed; a
/// damaged one is an error. `observer` hears the build; `None` draws and prints nothing.
///
/// # Errors
///
/// If the lists cannot be loaded, a stored book is damaged, or the book cannot be written.
pub fn ensure<const N: usize>(
    config: &RunConfig,
    data_dir: &DataDir,
    source: &Source,
    lists: &GameLists<N>,
    force: bool,
    observer: Option<&dyn Observer>,
) -> anyhow::Result<Ensured<N>> {
    let lang = language(&config.game);
    let key = key(config, source, lists);
    let path = data_dir.book_path(lang, &key);
    let mut stale_fields = None;
    let mut message = "rebuilding the opening book".to_owned();
    if !force {
        match data_dir.read_book(lang, &key)? {
            BookLookup::Found(stored) => {
                let bytes = std::fs::metadata(&path)?.len();
                let book = OpeningBook::<N>::from_nodes(stored, lists.pool.len(), true)
                    .with_context(|| {
                        format!(
                            "opening book {} is corrupt; rebuild it with `lex book build --force`",
                            path.display()
                        )
                    })?;
                return Ok(Ensured {
                    book,
                    status: Status::UpToDate,
                    stale_fields: None,
                    path,
                    bytes,
                });
            }
            BookLookup::Missing { nearest: None } => {
                message = format!("no opening book for {}; building it", source.name());
            }
            BookLookup::Missing {
                nearest: Some(fields),
            } => {
                message = format!(
                    "no opening book for this key (the closest on disk differs in {}); building it",
                    fields.join(", ")
                );
                stale_fields = Some(fields);
            }
        }
    }

    if let Some(observer) = observer {
        observer.book_started(&message);
    }
    let started = Instant::now();
    let guesser = Guesser::new(&lists.pool, &lists.answers, config.strategy);
    let book = guesser.build_book_reporting(&mut |booked| {
        if let Some(observer) = observer {
            observer.book_progress(booked);
        }
    });
    let elapsed = started.elapsed();
    let stored = book
        .to_nodes()
        .context("a book built with recording on holds its scores")?;
    let file = data_dir.write_book(lang, &key, &stored)?;
    if let Some(observer) = observer {
        observer.book_finished(&format!(
            "built the opening book: {} positions, {} bytes, {elapsed:.1?}",
            book.len(),
            file.bytes
        ));
    }
    Ok(Ensured {
        book,
        status: Status::Built(elapsed),
        stale_fields,
        path: file.path,
        bytes: file.bytes,
    })
}

/// A guesser for `lists`, seeded with the Opening book unless `mode` is off.
///
/// The book's techniques and scores stay in memory only when `scores` is set, for a caller that
/// will switch explaining on; plain play drops them as soon as the file is parsed.
///
/// # Errors
///
/// As [`ensure`], and when the stored book does not fit the Guess pool.
pub fn guesser<const N: usize>(
    config: &RunConfig,
    data_dir: &DataDir,
    source: &Source,
    lists: GameLists<N>,
    mode: BookMode,
    scores: bool,
    observer: Option<&dyn Observer>,
) -> anyhow::Result<Guesser<N>> {
    let strategy = config.strategy;
    if mode == BookMode::Off {
        let GameLists { pool, answers } = lists;
        return Ok(Guesser::new(&pool, &answers, strategy));
    }
    let ensured = ensure(config, data_dir, source, &lists, false, observer)?;
    let book = if scores {
        ensured.book
    } else {
        ensured.book.without_scores()
    };
    let GameLists { pool, answers } = lists;
    Ok(Guesser::with_book(&pool, &answers, strategy, book))
}

/// The `lex book build` command: builds the book, or reports that it is up to date.
///
/// # Errors
///
/// As [`ensure`].
pub fn build_command<const N: usize>(args: &BookBuildArgs) -> anyhow::Result<()> {
    let config = args.config();
    let data_dir = args.common.data();
    let (lists, source) = Source::load::<N>(&config, &data_dir)?;
    let observer = progress::observer();
    let ensured = ensure(
        &config,
        &data_dir,
        &source,
        &lists,
        args.force,
        observer.as_deref(),
    )?;
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
