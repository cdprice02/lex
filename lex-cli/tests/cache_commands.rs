//! The cache command groups: parsing, and the cascades of `clear` against a temp data dir.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::path::Path;

use clap::Parser;
use clap::error::ErrorKind;
use lex_cli::cli::{Cli, Command, CorpusCommand, DictCommand, ProfileCommand};

fn parse(args: &[&str]) -> Result<Command, clap::Error> {
    Cli::try_parse_from(std::iter::once("lex").chain(args.iter().copied())).map(|cli| cli.command)
}

fn kind(args: &[&str]) -> ErrorKind {
    parse(args).unwrap_err().kind()
}

/// Writes `data_dir/<relative>` with a stand-in body.
fn touch(data_dir: &Path, relative: &str) {
    let path = data_dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

/// A data dir holding a Dictionary, two Corpus lengths and a book for each of `codes`.
fn seeded(codes: &[&str]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for code in codes {
        for relative in [
            format!("dicts/{code}.txt"),
            format!("dicts/{code}.meta.json"),
            format!("ngrams/{code}/5.bin"),
            format!("ngrams/{code}/6.bin"),
            format!("books/{code}/5/a.bin"),
            format!("books/{code}/6/b.bin"),
        ] {
            touch(tmp.path(), &relative);
        }
    }
    tmp
}

fn run_dict_clear(tmp: &tempfile::TempDir, args: &[&str]) {
    let dir = tmp.path().to_str().unwrap();
    let Command::Dict(args) =
        parse(&[&["dict", "clear", "--data-dir", dir], args].concat()).unwrap()
    else {
        panic!("`dict clear` parses to the dict command")
    };
    let DictCommand::Clear(args) = args.command else {
        panic!("`dict clear` parses to the clear operation")
    };
    lex_cli::clear::dict(&args).unwrap();
}

#[test]
fn dict_clear_removes_the_dictionary_every_length_and_the_books_of_one_language() {
    let tmp = seeded(&["eng", "fre"]);
    run_dict_clear(&tmp, &["--lang", "english"]);
    let root = tmp.path();
    assert!(!root.join("dicts/eng.txt").exists());
    assert!(!root.join("dicts/eng.meta.json").exists());
    assert!(!root.join("ngrams/eng").exists());
    assert!(!root.join("books/eng").exists());
    for kept in [
        "dicts/fre.txt",
        "dicts/fre.meta.json",
        "ngrams/fre/5.bin",
        "ngrams/fre/6.bin",
        "books/fre/5/a.bin",
        "books/fre/6/b.bin",
    ] {
        assert!(root.join(kept).exists(), "{kept} survives");
    }
}

#[test]
fn dict_clear_all_removes_every_language() {
    let tmp = seeded(&["eng", "fre"]);
    run_dict_clear(&tmp, &["--all"]);
    for gone in ["dicts/eng.txt", "dicts/fre.txt", "ngrams/fre", "books/eng"] {
        assert!(!tmp.path().join(gone).exists(), "{gone}");
    }
}

#[test]
fn corpus_clear_all_keeps_dictionaries_and_removes_books() {
    let tmp = seeded(&["eng", "fre"]);
    let dir = tmp.path().to_str().unwrap();
    let Command::Corpus(args) = parse(&["corpus", "clear", "--all", "--data-dir", dir]).unwrap()
    else {
        panic!("`corpus clear` parses to the corpus command")
    };
    let CorpusCommand::Clear(args) = args.command else {
        panic!("parses to the clear operation")
    };
    lex_cli::clear::corpus(&args).unwrap();
    assert!(!tmp.path().join("ngrams/eng").exists());
    assert!(!tmp.path().join("ngrams/fre").exists());
    assert!(!tmp.path().join("books/fre").exists());
    assert!(tmp.path().join("dicts/eng.txt").exists());
}

#[test]
fn profile_clear_all_removes_fetched_profiles_and_skips_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "profiles/wordle/answers.txt");
    let dir = tmp.path().to_str().unwrap();
    let Command::Profile(args) = parse(&["profile", "clear", "--all", "--data-dir", dir]).unwrap()
    else {
        panic!("`profile clear` parses to the profile command")
    };
    let ProfileCommand::Clear(args) = args.command else {
        panic!("parses to the clear operation")
    };
    lex_cli::clear::profile(&args).unwrap();
    assert!(!tmp.path().join("profiles/wordle").exists());
}

#[test]
fn the_summary_lists_each_path_and_a_total() {
    let removed = [
        Path::new("data/a").to_path_buf(),
        Path::new("data/b").to_path_buf(),
    ];
    assert_eq!(
        lex_cli::clear::summary(&removed),
        "removed data/a\nremoved data/b\n2 paths removed"
    );
    assert_eq!(
        lex_cli::clear::summary(&removed[..1]).lines().last(),
        Some("1 path removed")
    );
}

#[test]
fn clear_with_nothing_named_is_an_error_unless_all() {
    for group in ["dict", "corpus", "profile", "book"] {
        assert_eq!(
            kind(&[group, "clear"]),
            ErrorKind::MissingRequiredArgument,
            "{group}"
        );
        assert!(parse(&[group, "clear", "--all"]).is_ok(), "{group}");
    }
}

#[test]
fn dict_clear_rejects_a_word_length() {
    assert_eq!(
        kind(&["dict", "clear", "--lang", "english", "-l", "5"]),
        ErrorKind::UnknownArgument
    );
}

#[test]
fn all_conflicts_with_naming_something() {
    assert_eq!(
        kind(&["dict", "clear", "--all", "--lang", "english"]),
        ErrorKind::ArgumentConflict
    );
    assert_eq!(
        kind(&["profile", "clear", "--all", "--profile", "wordle"]),
        ErrorKind::ArgumentConflict
    );
    assert_eq!(
        kind(&["corpus", "clear", "--all", "-l", "5"]),
        ErrorKind::ArgumentConflict
    );
}

#[test]
fn the_groups_parse_and_dictionary_is_an_alias() {
    assert!(matches!(
        parse(&["dictionary", "clear", "--lang", "english"]).unwrap(),
        Command::Dict(_)
    ));
    assert!(parse(&["corpus", "clear", "--lang", "english", "-l", "5"]).is_ok());
    assert!(parse(&["profile", "clear", "--profile", "wordle"]).is_ok());
    assert!(parse(&["book", "clear", "--lang", "english", "-l", "5"]).is_ok());
    assert!(parse(&["book", "build", "--lang", "english", "-l", "5"]).is_ok());
}

#[test]
fn corpus_and_profile_clear_of_an_absent_item_remove_nothing_and_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();
    for lang_args in [&["-l", "5"][..], &[][..]] {
        let args = [
            &["corpus", "clear", "--lang", "english", "--data-dir", dir][..],
            lang_args,
        ]
        .concat();
        let Command::Corpus(args) = parse(&args).unwrap() else {
            panic!("`corpus clear` parses to the corpus command")
        };
        let CorpusCommand::Clear(args) = args.command else {
            panic!("parses to the clear operation")
        };
        lex_cli::clear::corpus(&args).unwrap();
    }
    let Command::Profile(args) =
        parse(&["profile", "clear", "--profile", "wordle", "--data-dir", dir]).unwrap()
    else {
        panic!("`profile clear` parses to the profile command")
    };
    let ProfileCommand::Clear(args) = args.command else {
        panic!("parses to the clear operation")
    };
    lex_cli::clear::profile(&args).unwrap();
}

#[test]
fn the_old_clear_command_is_gone() {
    assert_eq!(
        kind(&["clear", "--lang", "english"]),
        ErrorKind::InvalidSubcommand
    );
}

/// Runs the `lex` binary with stdout piped (so output is the plain form).
fn lex(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_lex"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn dict_list_and_show_parse_and_reject_a_missing_language_for_show() {
    assert!(parse(&["dict", "list"]).is_ok());
    assert!(parse(&["dictionary", "list", "--lang", "english"]).is_ok());
    assert!(parse(&["dict", "show", "--lang", "english"]).is_ok());
    assert_eq!(kind(&["dict", "show"]), ErrorKind::MissingRequiredArgument);
}

#[test]
fn piped_dict_list_prints_a_header_and_a_row_per_cached_language_without_fetching() {
    let tmp = tempfile::tempdir().unwrap();
    for relative in ["dicts/eng.txt", "ngrams/eng/5.bin", "ngrams/eng/6.bin"] {
        touch(tmp.path(), relative);
    }
    let dir = tmp.path().to_str().unwrap();
    let out = lex(&["dict", "list", "--data-dir", dir]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(lines[0].starts_with("language\tcode\t"));
    assert!(lines[1].starts_with("english\teng\t"));
    assert!(lines[1].ends_with("\t5,6"));
}

#[test]
fn dict_list_with_nothing_cached_succeeds_and_show_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();
    let out = lex(&["dict", "list", "--data-dir", dir]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"no Dictionaries cached\n");
    let out = lex(&["dict", "show", "--lang", "english", "--data-dir", dir]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("lex dict fetch --lang english"), "{stderr}");
    assert!(!tmp.path().join("dicts").exists());
}

fn book_key(digest: &str, word_length: usize) -> lex_data::BookKey {
    lex_data::BookKey {
        corpus_sha256: "any".to_owned(),
        profile: None,
        word_length,
        dictionary_length: 10,
        strategy: vec![("pool".to_owned(), "hard".to_owned())],
        solver_digest: digest.to_owned(),
    }
}

/// A data dir with a current, a stale and a corrupt English book and a stale French one.
/// Returns the ids of the current and the stale English books.
fn books_dir() -> (tempfile::TempDir, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = lex_data::DataDir::new(tmp.path());
    let book = lex_data::StoredBook::default();
    let current = book_key(lex_core::SOLVER_DIGEST, 5);
    let stale = book_key("an old digest", 5);
    dir.write_book(lex_data::Language::English, &current, &book)
        .unwrap();
    dir.write_book(lex_data::Language::English, &stale, &book)
        .unwrap();
    dir.write_book(
        lex_data::Language::French,
        &book_key("an old digest", 6),
        &book,
    )
    .unwrap();
    std::fs::write(tmp.path().join("books/eng/5/junk.bin"), b"not a book").unwrap();
    (tmp, current.hash(), stale.hash())
}

#[test]
fn book_list_show_and_clear_stale_parse() {
    assert!(parse(&["book", "list", "--lang", "english", "-l", "5", "--stale"]).is_ok());
    assert!(parse(&["book", "list", "--profile", "wordle"]).is_ok());
    assert!(parse(&["book", "show", "abcd1234"]).is_ok());
    assert!(
        parse(&[
            "book", "show", "-l", "5", "--lang", "english", "--pool", "hard"
        ])
        .is_ok()
    );
    assert!(parse(&["book", "show", "--profile", "wordle"]).is_ok());
    assert_eq!(kind(&["book", "show"]), ErrorKind::MissingRequiredArgument);
    assert_eq!(
        kind(&["book", "show", "abcd1234", "--pool", "hard"]),
        ErrorKind::ArgumentConflict
    );
    assert!(parse(&["book", "clear", "--stale"]).is_ok());
    assert!(parse(&["book", "clear", "--stale", "--profile", "wordle"]).is_ok());
    assert!(parse(&["book", "clear", "--stale", "--lang", "english", "-l", "5"]).is_ok());
    assert_eq!(
        kind(&["book", "clear", "--profile", "wordle", "--lang", "english"]),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        kind(&["book", "clear", "--stale", "--all"]),
        ErrorKind::ArgumentConflict
    );
    assert_eq!(kind(&["book", "clear"]), ErrorKind::MissingRequiredArgument);
}

#[test]
fn piped_book_list_has_a_header_a_row_per_book_and_the_footer_on_stderr() {
    let (tmp, current, _) = books_dir();
    let dir = tmp.path().to_str().unwrap();
    let out = lex(&["book", "list", "--data-dir", dir]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 5, "{stdout}");
    assert_eq!(lines[0], "id\tgame\tstrategy\tpositions\tsize\tstatus");
    let row = lines
        .iter()
        .find(|l| l.starts_with(current.get(..8).unwrap()))
        .unwrap();
    assert!(row.contains("\tenglish-5\tpool=hard\t0\t"), "{row}");
    assert!(row.ends_with("\tcurrent"), "{row}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.starts_with("4 books: 1 current, 2 stale, 1 corrupt; total size "),
        "{stderr}"
    );
}

#[test]
fn book_list_filters_and_stale_lists_what_clear_would_remove() {
    let (tmp, _, _) = books_dir();
    let dir = tmp.path().to_str().unwrap();
    let rows = |extra: &[&str]| {
        let out = lex(&[&["book", "list", "--data-dir", dir], extra].concat());
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().lines().count() - 1
    };
    assert_eq!(rows(&["--lang", "english"]), 3);
    assert_eq!(rows(&["--lang", "french"]), 1);
    assert_eq!(rows(&["-l", "6"]), 1);
    assert_eq!(rows(&["--stale"]), 3);
    assert_eq!(rows(&["--stale", "--lang", "english"]), 2);
    assert_eq!(rows(&["--profile", "wordle"]), 0);
}

#[test]
fn book_show_by_id_prefix_prints_every_key_field_and_the_stale_pair() {
    let (tmp, current, stale) = books_dir();
    let dir = tmp.path().to_str().unwrap();
    let out = lex(&["book", "show", stale.get(..10).unwrap(), "--data-dir", dir]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("field\tvalue\tnote\n"));
    assert!(stdout.contains("\nbody checksum\tverified\t\n"), "{stdout}");
    assert!(
        stdout.contains("\nstatus\tstale (solver_digest)\t"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "\nsolver_digest\tan old digest\tstale: current is {}\n",
            lex_core::SOLVER_DIGEST
        )),
        "{stdout}"
    );
    assert!(stdout.contains("\nstrategy.pool\thard\t\n"), "{stdout}");
    let out = lex(&["book", "show", &current, "--data-dir", dir]);
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("\nstatus\tcurrent\t")
    );
    let out = lex(&["book", "show", "zzzz", "--data-dir", dir]);
    assert!(!out.status.success());
}

#[test]
fn book_clear_stale_removes_stale_and_corrupt_books_inside_the_narrowing_only() {
    let (tmp, current, stale) = books_dir();
    let dir = tmp.path().to_str().unwrap();
    let out = lex(&[
        "book",
        "clear",
        "--stale",
        "--lang",
        "english",
        "--data-dir",
        dir,
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.ends_with("2 paths removed\n"), "{stdout}");
    let root = tmp.path().join("books");
    assert!(root.join(format!("eng/5/{current}.bin")).exists());
    assert!(!root.join(format!("eng/5/{stale}.bin")).exists());
    assert!(!root.join("eng/5/junk.bin").exists());
    assert_eq!(std::fs::read_dir(root.join("fre/6")).unwrap().count(), 1);
    let out = lex(&["book", "clear", "--stale", "--data-dir", dir]);
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .ends_with("1 path removed\n")
    );
    assert!(root.join(format!("eng/5/{current}.bin")).exists());
}

#[test]
fn corpus_list_filters_and_corpus_show_needs_a_language_and_length() {
    let Command::Corpus(args) = parse(&["corpus", "list", "--lang", "english", "-l", "5"]).unwrap()
    else {
        panic!("`corpus list` parses to the corpus command")
    };
    let CorpusCommand::List(args) = args.command else {
        panic!("parses to the list operation")
    };
    assert_eq!(args.word_length, Some(5));
    assert!(parse(&["corpus", "list"]).is_ok());
    assert_eq!(
        kind(&["corpus", "show", "--lang", "english"]),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        kind(&["corpus", "show", "-l", "5"]),
        ErrorKind::MissingRequiredArgument
    );
    assert!(parse(&["corpus", "show", "--lang", "english", "-l", "5"]).is_ok());
}

#[test]
fn profile_show_needs_a_profile_and_list_takes_none() {
    assert!(parse(&["profile", "list"]).is_ok());
    assert_eq!(
        kind(&["profile", "show"]),
        ErrorKind::MissingRequiredArgument
    );
    assert!(parse(&["profile", "show", "--profile", "wordle-all"]).is_ok());
}

#[test]
fn corpus_and_profile_list_create_nothing_in_an_empty_data_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();
    let Command::Corpus(args) = parse(&["corpus", "list", "--data-dir", dir]).unwrap() else {
        panic!("corpus")
    };
    let CorpusCommand::List(args) = args.command else {
        panic!("list")
    };
    lex_cli::corpora::list(&args).unwrap();
    let Command::Profile(args) = parse(&["profile", "list", "--data-dir", dir]).unwrap() else {
        panic!("profile")
    };
    let ProfileCommand::List(args) = args.command else {
        panic!("list")
    };
    lex_cli::profiles::list(&args).unwrap();
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}
