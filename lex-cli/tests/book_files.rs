//! Opening book files through the CLI's own seams: the first use builds, a changed key rebuilds
//! and says why, damage fails loudly, and `lex clear` prunes. The round trip of a non-empty
//! book is `lex-core`'s `guesser` test.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

mod common;

use clap::Parser;
use common::{seed_dict, seed_supported};
use lex_cli::books::{self, BookMode, Status};
use lex_cli::cli::{Cli, Command};
use lex_cli::config::{Game, RunConfig, Targets};
use lex_cli::lists::Source;
use lex_cli::report::Report;
use lex_cli::simulate::{run, run_with};
use lex_core::Strategy;
use lex_data::{DataDir, Language};

const WORDS: [(&str, u64); 8] = [
    ("crane", 900),
    ("stare", 800),
    ("light", 700),
    ("mount", 600),
    ("swipe", 500),
    ("slate", 400),
    ("grate", 300),
    ("plate", 200),
];

fn seeded() -> (tempfile::TempDir, DataDir) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_supported(&dir, Language::English, 5, &WORDS);
    seed_dict(&dir, Language::English);
    (tmp, dir)
}

const fn config(strategy: Strategy) -> RunConfig {
    RunConfig {
        game: Game::Corpus {
            language: Language::English,
            length: 5,
            dictionary_length: None,
        },
        strategy,
        targets: Targets::Full,
    }
}

fn ensure(dir: &DataDir, config: &RunConfig, force: bool) -> anyhow::Result<books::Ensured<5>> {
    let (lists, source) = Source::load::<5>(config, dir)?;
    books::ensure(config, dir, &source, &lists, force)
}

fn games(report: &Report) -> Vec<(String, Vec<String>)> {
    report
        .games
        .iter()
        .map(|g| (g.word.clone(), g.guesses.clone()))
        .collect()
}

fn book_files(dir: &DataDir) -> Vec<std::path::PathBuf> {
    let root = dir.as_ref().join("books/eng/5");
    std::fs::read_dir(root)
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default()
}

#[test]
fn the_first_use_builds_and_the_second_loads() {
    let (_tmp, dir) = seeded();
    let config = config(Strategy::default());
    assert!(book_files(&dir).is_empty());

    let first = ensure(&dir, &config, false).unwrap();
    assert!(matches!(first.status, Status::Built(_)));
    assert_eq!(first.stale_fields, None);
    assert_eq!(book_files(&dir), std::slice::from_ref(&first.path));

    let second = ensure(&dir, &config, false).unwrap();
    assert_eq!(second.status, Status::UpToDate);
    assert_eq!(second.book, first.book);
    assert_eq!(second.bytes, first.bytes);
}

#[test]
fn force_rebuilds_an_up_to_date_book_in_place() {
    let (_tmp, dir) = seeded();
    let config = config(Strategy::default());
    let first = ensure(&dir, &config, false).unwrap();
    let forced = ensure(&dir, &config, true).unwrap();
    assert!(matches!(forced.status, Status::Built(_)));
    assert_eq!(forced.path, first.path);
    assert_eq!(book_files(&dir).len(), 1);
}

#[test]
fn a_changed_strategy_field_builds_a_second_book_and_names_the_field() {
    let (_tmp, dir) = seeded();
    ensure(&dir, &config(Strategy::default()), false).unwrap();
    let other = Strategy {
        temperature: 4.0,
        ..Strategy::default()
    };
    let rebuilt = ensure(&dir, &config(other), false).unwrap();
    assert!(matches!(rebuilt.status, Status::Built(_)));
    assert_eq!(
        rebuilt.stale_fields,
        Some(vec!["strategy.temperature".to_owned()])
    );
    assert_eq!(
        book_files(&dir).len(),
        2,
        "books for both strategies coexist"
    );
}

#[test]
fn a_changed_corpus_rebuilds_and_names_its_checksum() {
    let (_tmp, dir) = seeded();
    let config = config(Strategy::default());
    ensure(&dir, &config, false).unwrap();
    let mut words = WORDS.to_vec();
    words[0].1 += 1;
    seed_supported(&dir, Language::English, 5, &words);
    let rebuilt = ensure(&dir, &config, false).unwrap();
    assert_eq!(rebuilt.stale_fields, Some(vec!["corpus_sha256".to_owned()]));
}

#[test]
fn a_damaged_book_is_an_error_naming_the_file_and_is_not_rebuilt() {
    let (_tmp, dir) = seeded();
    let config = config(Strategy::default());
    let first = ensure(&dir, &config, false).unwrap();
    let mut bytes = std::fs::read(&first.path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&first.path, &bytes).unwrap();

    let err = format!("{:#}", ensure(&dir, &config, false).err().unwrap());
    assert!(err.contains(first.path.to_str().unwrap()), "{err}");
    assert!(err.contains("lex book build --force"), "{err}");
    assert_eq!(std::fs::read(&first.path).unwrap(), bytes, "left as found");

    let repaired = ensure(&dir, &config, true).unwrap();
    assert!(matches!(repaired.status, Status::Built(_)));
}

#[test]
fn a_run_builds_the_book_unless_told_not_to_and_plays_the_same_games() {
    let (_tmp, dir) = seeded();
    let config = config(Strategy::default());

    let online = run_with(&config, &dir, BookMode::Off).unwrap();
    assert!(book_files(&dir).is_empty(), "--no-book writes nothing");

    let booked = run(&config, &dir).unwrap();
    assert_eq!(book_files(&dir).len(), 1);
    assert_eq!(games(&booked), games(&online));
    assert_eq!(games(&run(&config, &dir).unwrap()), games(&online));
}

fn clear(dir: &DataDir, args: &[&str]) {
    let Command::Clear(args) = Cli::try_parse_from(
        ["lex", "clear", "--data-dir", dir.as_ref().to_str().unwrap()]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .unwrap()
    .command
    else {
        panic!("`clear` parses to the clear command")
    };
    lex_cli::clear::clear(&args).unwrap();
}

#[test]
fn clear_books_removes_books_and_nothing_else() {
    let (_tmp, dir) = seeded();
    ensure(&dir, &config(Strategy::default()), false).unwrap();
    clear(&dir, &["--books", "--lang", "english", "-l", "5"]);
    assert!(book_files(&dir).is_empty());
    assert!(dir.ngrams_path(Language::English, 5).exists());
}

#[test]
fn clearing_a_corpus_removes_its_books() {
    let (_tmp, dir) = seeded();
    ensure(&dir, &config(Strategy::default()), false).unwrap();
    clear(&dir, &["--lang", "english", "-l", "5"]);
    assert!(book_files(&dir).is_empty());
    assert!(!dir.ngrams_path(Language::English, 5).exists());
}
