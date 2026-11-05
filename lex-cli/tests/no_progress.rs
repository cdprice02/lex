//! `--no-progress` removes every progress byte: with stderr piped, a run that builds its book and
//! plays games writes nothing to stderr.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

mod common;

use std::process::Command;

use common::{seed_dict, seed_supported};
use lex_data::{DataDir, Language};

/// Runs `lex simulate -n 3` over seeded data with `extra` flags, stdout and stderr piped.
fn simulate(extra: &[&str]) -> std::process::Output {
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
        ],
    );
    seed_dict(&dir, Language::English);
    Command::new(env!("CARGO_BIN_EXE_lex"))
        .args([
            "simulate",
            "-l",
            "5",
            "--lang",
            "english",
            "-n",
            "3",
            "--data-dir",
        ])
        .arg(tmp.path())
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn no_progress_leaves_stderr_empty() {
    let output = simulate(&["--no-progress"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert!(String::from_utf8_lossy(&output.stdout).contains("3 games"));
}

#[test]
fn off_a_terminal_the_book_build_reports_as_lines() {
    let output = simulate(&[]);
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("built the opening book"), "{stderr}");
}
