//! The `--track` flag plays and builds a Track in-process.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

mod common;

use clap::Parser;
use common::{seed_dict, seed_supported};
use lex_cli::cli::{Cli, Command};
use lex_cli::report::Report;
use lex_cli::simulate::run;
use lex_cli::tracks::Track;
use lex_data::{DataDir, Language};

/// The report as JSON, without the run timestamp, the one field that differs between runs.
fn bytes(mut report: Report) -> String {
    report.run.timestamp.clear();
    serde_json::to_string_pretty(&report).unwrap()
}

#[test]
fn track_flag_and_run_of_its_config_give_equal_reports() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_supported(
        &dir,
        Language::English,
        5,
        &[
            ("crane", 900),
            ("stare", 800),
            ("light", 700),
            ("mount", 600),
            ("swipe", 500),
            ("slate", 400),
            ("grate", 300),
            ("plate", 200),
        ],
    );

    seed_dict(&dir, Language::English);
    let data_dir = tmp.path().to_str().unwrap();
    let Command::Simulate(args) = Cli::try_parse_from([
        "lex",
        "simulate",
        "--track",
        "english-5",
        "--data-dir",
        data_dir,
    ])
    .unwrap()
    .command
    else {
        panic!("`simulate` parses to the simulate command")
    };

    let track = Track::find("english-5").unwrap();
    let from_flag = run(&args.config(), &dir).unwrap();
    let from_config = run(&track.config(), &dir).unwrap();
    assert_eq!(from_flag.games.len(), 200);
    assert_eq!(bytes(from_flag), bytes(from_config));
}
