//! `dict fetch`, `corpus build` and `profile fetch` against a temp data dir and a stubbed
//! [`Network`]: what each downloads, the upward cascade, idempotence and the `--force` cascades.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::cell::RefCell;
use std::path::Path;

use clap::Parser;
use clap::error::ErrorKind;
use lex_cli::cli::{Cli, Command, CorpusCommand, DictCommand, ProfileCommand};
use lex_cli::fetch::{Network, Online, Sinks, corpus_with, dict_with, profile_with};
use lex_data::testing::{seed_dict, seed_supported, seed_totals};
use lex_data::{DataDir, Language, Profile};
use strum::VariantArray as _;

/// Records what was downloaded and writes stand-in files in place of the network.
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<String>>,
    /// The download workers each `corpus` call's `DataDir` carried.
    workers: RefCell<Vec<Option<usize>>>,
}

impl Fake {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl Network for Fake {
    fn dict(&self, dir: &DataDir, lang: Language) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(format!("dict {lang}"));
        let path = dir.dict_path(lang);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, "crane\n")?;
        std::fs::write(
            path.with_extension("meta.json"),
            r#"{"min_word_length":5,"max_word_length":5,"word_count":1,"source_url":"https://example.test/x.gz","fetched_at":"2026-05-02T00:00:00Z","source_sha256":"abcdef0123456789"}"#,
        )?;
        Ok(())
    }

    fn corpus(
        &self,
        dir: &DataDir,
        lang: Language,
        progress: &(dyn Fn(u32, u32) + Sync),
    ) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(format!("corpus {lang}"));
        self.workers
            .borrow_mut()
            .push(dir.download_workers().map(std::num::NonZeroUsize::get));
        progress(0, 1);
        seed_supported(dir, lang, 5, &[("crane", 900), ("stare", 800)]);
        progress(1, 1);
        Ok(())
    }

    fn profile(&self, dir: &DataDir, profile: Profile) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(format!("profile {profile}"));
        let root = dir.profile_dir(profile);
        std::fs::create_dir_all(&root)?;
        std::fs::write(root.join("answers.txt"), "crane\n")?;
        std::fs::write(root.join("guess-only.txt"), "stare\n")?;
        std::fs::write(
            root.join("meta.json"),
            r#"{"profile":"wordle","fetched_at":"2026-05-03T00:00:00Z"}"#,
        )?;
        Ok(())
    }
}

/// What a command printed.
struct Printed {
    out: String,
    warn: String,
}

/// Parses `args` after `lex` and returns the command.
fn parse(args: &[&str]) -> Result<Command, clap::Error> {
    Cli::try_parse_from(std::iter::once("lex").chain(args.iter().copied())).map(|cli| cli.command)
}

/// Runs `lex <group> <verb> <args> --data-dir <root>` over `net`.
fn run(net: &Fake, root: &Path, group_verb: &[&str], args: &[&str]) -> Printed {
    let data = root.to_str().unwrap();
    let all: Vec<&str> = group_verb
        .iter()
        .chain(args)
        .copied()
        .chain(["--data-dir", data])
        .collect();
    let (mut out, mut warn) = (Vec::new(), Vec::new());
    {
        let mut sinks = Sinks {
            out: &mut out,
            warn: &mut warn,
            terminal: false,
        };
        let command = parse(&all).unwrap();
        if let Command::Dict(a) = command {
            let DictCommand::Fetch(a) = a.command else {
                panic!("dict fetch")
            };
            dict_with(&a, net, &mut sinks).unwrap();
        } else if let Command::Corpus(a) = command {
            let CorpusCommand::Build(a) = a.command else {
                panic!("corpus build")
            };
            corpus_with(&a, net, &mut sinks).unwrap();
        } else if let Command::Profile(a) = command {
            let ProfileCommand::Fetch(a) = a.command else {
                panic!("profile fetch")
            };
            profile_with(&a, net, &mut sinks).unwrap();
        } else {
            panic!("a cache command");
        }
    }
    Printed {
        out: String::from_utf8(out).unwrap(),
        warn: String::from_utf8(warn).unwrap(),
    }
}

/// Writes a stand-in file under `root`.
fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

const DICT: &[&str] = &["dict", "fetch"];
const CORPUS: &[&str] = &["corpus", "build"];
const PROFILE: &[&str] = &["profile", "fetch"];

#[test]
fn dict_fetch_downloads_then_prints_the_show_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let printed = run(&net, tmp.path(), DICT, &["--lang", "english"]);
    assert_eq!(net.calls(), ["dict english"]);
    assert!(printed.out.contains("fetched the english Dictionary\n"));
    assert!(printed.out.contains("field\tvalue\tnote\n"));
    assert!(printed.out.contains("\nfetched at\t2026-05-02T00:00:00Z\t"));
    assert_eq!(printed.warn, "");
}

#[test]
fn a_cached_dictionary_is_ready_and_nothing_is_downloaded() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    run(&net, tmp.path(), DICT, &["--lang", "english"]);
    let again = run(&net, tmp.path(), DICT, &["--lang", "english"]);
    assert_eq!(net.calls().len(), 1, "the second run is a no-op");
    assert!(
        again
            .out
            .contains("english Dictionary is ready (fetched 2026-05-02)"),
        "{}",
        again.out
    );
    assert!(again.out.contains("field\tvalue\tnote\n"));
}

#[test]
fn dict_fetch_force_warns_clears_the_corpus_and_books_then_fetches() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_dict(&dir, Language::English);
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    touch(tmp.path(), "books/eng/5/a.bin");
    seed_dict(&dir, Language::French);
    touch(tmp.path(), "books/fre/5/a.bin");
    let net = Fake::default();
    let printed = run(&net, tmp.path(), DICT, &["--lang", "english", "--force"]);
    assert_eq!(net.calls(), ["dict english"]);
    assert!(printed.warn.contains("rolling"), "{}", printed.warn);
    assert!(!tmp.path().join("ngrams/eng").exists());
    assert!(!tmp.path().join("books/eng").exists());
    assert!(tmp.path().join("books/fre/5/a.bin").exists());
    assert!(tmp.path().join("dicts/fre.txt").exists());
}

#[test]
fn dict_fetch_all_covers_every_language() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    run(&net, tmp.path(), DICT, &["--all"]);
    assert_eq!(net.calls().len(), lex_data::Language::VARIANTS.len());
}

#[test]
fn corpus_build_fetches_the_missing_dictionary_first() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let printed = run(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert_eq!(net.calls(), ["dict english", "corpus english"]);
    assert!(printed.out.contains("built the english Corpus\n"));
    assert!(
        printed
            .out
            .contains("language\tlength\twords\tsupported\tsize\tchecksum\tstatus\ttotals\n"),
        "{}",
        printed.out
    );
    assert!(printed.out.contains("english\t5\t"));
}

#[test]
fn a_cached_corpus_is_ready_and_nothing_is_downloaded() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    let printed = run(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert!(net.calls().is_empty());
    assert!(
        printed
            .out
            .contains("english Corpus is ready (1 lengths cached)"),
        "{}",
        printed.out
    );
}

/// An English Dictionary of two five-letter words and one six-letter word, with Ngrams totals
/// that also hold a word of length 7 the Dictionary lacks.
fn english_with_totals(root: &Path) -> DataDir {
    let dir = DataDir::new(root);
    std::fs::create_dir_all(dir.dict_path(Language::English).parent().unwrap()).unwrap();
    std::fs::write(dir.dict_path(Language::English), "crane\nplanet\nstare\n").unwrap();
    seed_totals(
        &dir,
        Language::English,
        &[
            (5, &[("crane", 9), ("stare", 8), ("zzzzz", 7)]),
            (6, &[("planet", 5)]),
            (7, &[("qqqqqqq", 3)]),
        ],
    );
    dir
}

#[test]
fn with_totals_ready_means_every_length_the_totals_give_is_cached() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = english_with_totals(tmp.path());
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    let printed = run(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert_eq!(net.calls(), ["corpus english"], "length 6 is missing");
    assert!(
        printed.out.contains("lacks the lengths 6"),
        "{}",
        printed.out
    );

    seed_supported(&dir, Language::English, 6, &[("planet", 9)]);
    let net = Fake::default();
    let printed = run(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert!(net.calls().is_empty(), "7 has no Dictionary word: ready");
    assert!(
        printed
            .out
            .contains("english Corpus is ready (2 lengths cached)"),
        "{}",
        printed.out
    );
}

#[test]
fn without_totals_any_cached_length_is_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    std::fs::create_dir_all(dir.dict_path(Language::English).parent().unwrap()).unwrap();
    std::fs::write(dir.dict_path(Language::English), "crane\nplanet\n").unwrap();
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    let printed = run(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert!(net.calls().is_empty());
    assert!(printed.out.contains("is ready (1 lengths cached)"));
}

#[test]
fn corpus_build_force_keeps_the_totals() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = english_with_totals(tmp.path());
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    let printed = run(&net, tmp.path(), CORPUS, &["--lang", "english", "--force"]);
    assert!(dir.totals(Language::English).unwrap().is_some());
    assert!(
        printed.out.contains("Ngrams totals are cached"),
        "{}",
        printed.out
    );
}

#[test]
fn corpus_build_force_clears_corpus_and_books_and_keeps_the_dictionary() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_dict(&dir, Language::English);
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    touch(tmp.path(), "ngrams/eng/6.bin");
    touch(tmp.path(), "books/eng/5/a.bin");
    let net = Fake::default();
    run(&net, tmp.path(), CORPUS, &["--lang", "english", "--force"]);
    assert_eq!(net.calls(), ["corpus english"], "no Dictionary fetch");
    assert!(
        !tmp.path().join("ngrams/eng/6.bin").exists(),
        "the old set is gone"
    );
    assert!(!tmp.path().join("books/eng").exists());
    assert!(tmp.path().join("ngrams/eng/5.bin").exists());
}

#[test]
fn profile_fetch_builds_the_corpus_and_the_dictionary_beneath_it() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let printed = run(&net, tmp.path(), PROFILE, &["--profile", "wordle"]);
    assert_eq!(
        net.calls(),
        ["dict english", "corpus english", "profile wordle"]
    );
    assert!(printed.out.contains("fetched Profile wordle\n"));
    assert!(printed.out.contains("\nprofile\twordle\t"));
}

#[test]
fn profile_fetch_with_a_cached_corpus_fetches_only_the_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    run(&net, tmp.path(), PROFILE, &["--profile", "wordle"]);
    assert_eq!(net.calls(), ["profile wordle"]);
}

#[test]
fn a_fetched_profile_is_ready_and_force_clears_its_books_before_refetching() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    run(&net, tmp.path(), PROFILE, &["--profile", "wordle"]);
    let ready = run(&net, tmp.path(), PROFILE, &["--profile", "wordle"]);
    assert_eq!(net.calls().len(), 1);
    assert!(
        ready
            .out
            .contains("Profile wordle is ready (fetched 2026-05-03)"),
        "{}",
        ready.out
    );
    touch(tmp.path(), "books/eng/5/a.bin");
    run(
        &net,
        tmp.path(),
        PROFILE,
        &["--profile", "wordle", "--force"],
    );
    assert_eq!(net.calls().len(), 2);
    assert!(tmp.path().join("profiles/wordle/meta.json").exists());
}

#[test]
fn a_word_length_is_rejected_and_a_selector_is_required() {
    for verb in [DICT, CORPUS] {
        let with_length: Vec<&str> = verb
            .iter()
            .copied()
            .chain(["--lang", "english", "-l", "5"])
            .collect();
        assert_eq!(
            parse(&with_length).unwrap_err().kind(),
            ErrorKind::UnknownArgument
        );
    }
    for verb in [DICT, CORPUS, PROFILE] {
        assert_eq!(
            parse(verb).unwrap_err().kind(),
            ErrorKind::MissingRequiredArgument
        );
    }
    assert_eq!(
        parse(&["dict", "fetch", "--lang", "english", "--all"])
            .unwrap_err()
            .kind(),
        ErrorKind::ArgumentConflict
    );
}

#[test]
fn the_binary_reports_cached_items_ready_without_a_network() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_dict(&dir, Language::English);
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let data = tmp.path().to_str().unwrap();
    for args in [
        ["dict", "fetch", "--lang", "english"],
        ["corpus", "build", "--lang", "english"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_lex"))
            .args(args)
            .args(["--no-progress", "--data-dir", data])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("is ready"));
        assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    }
}

/// The download workers `corpus build` and `profile fetch` hand to the Corpus build, with `main`'s
/// copy of the request into the arguments done by hand.
fn workers_of(group_verb: &[&str], request: Option<usize>) -> Vec<Option<usize>> {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let root = tmp.path().to_str().unwrap();
    let all: Vec<&str> = group_verb
        .iter()
        .copied()
        .chain(["--all", "--data-dir", root])
        .collect();
    let workers = request.and_then(std::num::NonZeroUsize::new);
    let mut sinks = Sinks {
        out: &mut Vec::new(),
        warn: &mut Vec::new(),
        terminal: false,
    };
    let command = parse(&all).unwrap();
    if let Command::Corpus(a) = command {
        let CorpusCommand::Build(mut a) = a.command else {
            panic!("corpus build")
        };
        a.download_workers = workers;
        corpus_with(&a, &net, &mut sinks).unwrap();
    } else if let Command::Profile(a) = command {
        let ProfileCommand::Fetch(mut a) = a.command else {
            panic!("profile fetch")
        };
        a.download_workers = workers;
        profile_with(&a, &net, &mut sinks).unwrap();
    } else {
        panic!("a fetch command");
    }
    net.workers.borrow().clone()
}

#[test]
fn corpus_build_hands_the_download_request_to_the_build() {
    let seen = workers_of(CORPUS, Some(7));
    assert_eq!(seen.len(), Language::VARIANTS.len());
    assert!(seen.iter().all(|w| *w == Some(7)));
    assert!(workers_of(CORPUS, None).iter().all(Option::is_none));
}

#[test]
fn profile_fetch_hands_the_download_request_to_the_corpus_build() {
    let seen = workers_of(PROFILE, Some(2));
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|w| *w == Some(2)));
}

/// Runs a fetch command with `--offline` (copied into the arguments as `main` does) over `net`,
/// returning what it printed and its result.
fn run_offline(
    net: &dyn Network,
    root: &Path,
    group_verb: &[&str],
    args: &[&str],
) -> (String, anyhow::Result<()>) {
    let data = root.to_str().unwrap();
    let all: Vec<&str> = group_verb
        .iter()
        .chain(args)
        .copied()
        .chain(["--data-dir", data])
        .collect();
    let mut out = Vec::new();
    let result = {
        let mut sinks = Sinks {
            out: &mut out,
            warn: &mut Vec::new(),
            terminal: false,
        };
        match parse(&all).unwrap() {
            Command::Dict(a) => {
                let DictCommand::Fetch(mut a) = a.command else {
                    panic!("dict fetch")
                };
                a.offline = true;
                dict_with(&a, net, &mut sinks)
            }
            Command::Corpus(a) => {
                let CorpusCommand::Build(mut a) = a.command else {
                    panic!("corpus build")
                };
                a.offline = true;
                corpus_with(&a, net, &mut sinks)
            }
            Command::Profile(a) => {
                let ProfileCommand::Fetch(mut a) = a.command else {
                    panic!("profile fetch")
                };
                a.offline = true;
                profile_with(&a, net, &mut sinks)
            }
            Command::Simulate(_)
            | Command::Assist(_)
            | Command::Compare(_)
            | Command::Book(_)
            | Command::Verdict(_)
            | Command::Word(_) => panic!("a fetch command"),
        }
    };
    (String::from_utf8(out).unwrap(), result)
}

fn offline_message(result: anyhow::Result<()>) -> String {
    let err = result.unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<lex_data::LexDataError>(),
            Some(lex_data::LexDataError::Offline { .. })
        ),
        "{err:?}"
    );
    err.to_string()
}

#[test]
fn offline_a_missing_dictionary_is_the_offline_error_and_nothing_is_downloaded() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let (_, result) = run_offline(&net, tmp.path(), DICT, &["--lang", "english"]);
    assert_eq!(
        offline_message(result),
        "offline: the english Dictionary is not cached; run `lex dict fetch --lang english`"
    );
    assert!(net.calls().is_empty());
}

#[test]
fn offline_a_cached_dictionary_is_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    run(&net, tmp.path(), DICT, &["--lang", "english"]);
    let (out, result) = run_offline(&net, tmp.path(), DICT, &["--lang", "english"]);
    result.unwrap();
    assert!(out.contains("english Dictionary is ready"), "{out}");
    assert_eq!(net.calls().len(), 1);
}

#[test]
fn offline_corpus_build_without_a_dictionary_or_totals_is_the_offline_error() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let (_, result) = run_offline(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert!(offline_message(result).contains("lex dict fetch --lang english"));

    let dir = DataDir::new(tmp.path());
    std::fs::create_dir_all(dir.dict_path(Language::English).parent().unwrap()).unwrap();
    std::fs::write(dir.dict_path(Language::English), "crane\n").unwrap();
    let (_, result) = run_offline(&net, tmp.path(), CORPUS, &["--lang", "english"]);
    assert!(offline_message(result).contains("lex corpus build --lang english"));
    assert!(net.calls().is_empty());
}

#[test]
fn offline_corpus_build_from_cached_totals_still_builds() {
    let tmp = tempfile::tempdir().unwrap();
    english_with_totals(tmp.path());
    let (out, result) = run_offline(&Online, tmp.path(), CORPUS, &["--lang", "english"]);
    result.unwrap();
    assert!(out.contains("built the english Corpus"), "{out}");
    let dir = DataDir::new(tmp.path());
    assert!(dir.ngrams_path(Language::English, 5).is_file());
    assert!(dir.ngrams_path(Language::English, 6).is_file());
    assert!(!dir.ngrams_path(Language::English, 7).exists());
}

#[test]
fn offline_profile_fetch_without_lists_is_the_offline_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_supported(&dir, Language::English, 5, &[("crane", 9)]);
    let net = Fake::default();
    let (_, result) = run_offline(&net, tmp.path(), PROFILE, &["--profile", "wordle"]);
    assert_eq!(
        offline_message(result),
        "offline: the lists of Profile wordle is not cached; run `lex profile fetch --profile wordle`"
    );
    assert!(net.calls().is_empty());
}

#[test]
fn offline_profile_fetch_without_a_corpus_names_the_dictionary() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Fake::default();
    let (_, result) = run_offline(&net, tmp.path(), PROFILE, &["--profile", "wordle"]);
    assert!(offline_message(result).contains("lex dict fetch --lang english"));
    assert!(net.calls().is_empty());
}

#[test]
fn force_conflicts_with_offline_in_all_three_commands_and_offline_is_global() {
    for verb in [DICT, CORPUS, PROFILE] {
        let selector = if verb == PROFILE {
            ["--profile", "wordle"]
        } else {
            ["--lang", "english"]
        };
        let args: Vec<&str> = verb
            .iter()
            .copied()
            .chain(selector)
            .chain(["--force", "--offline"])
            .collect();
        assert_eq!(
            parse(&args).unwrap_err().kind(),
            ErrorKind::ArgumentConflict,
            "{verb:?}"
        );
        let args: Vec<&str> = std::iter::once("--offline")
            .chain(verb.iter().copied())
            .chain(selector)
            .collect();
        assert!(parse(&args).is_ok(), "{verb:?}");
    }
}

#[test]
fn the_binary_offline_fails_with_the_fetch_command_and_exit_1() {
    let tmp = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lex"))
        .args([
            "--offline",
            "--no-progress",
            "dict",
            "fetch",
            "--lang",
            "english",
        ])
        .args(["--data-dir", tmp.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(
            "offline: the english Dictionary is not cached; run `lex dict fetch --lang english`"
        ),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}
