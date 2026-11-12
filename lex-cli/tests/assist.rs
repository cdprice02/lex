//! `lex assist` end to end: a scripted game piped into the binary over a small seeded corpus.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::io::Write;
use std::process::{Command, Stdio};

use lex_data::testing::{seed_dict, seed_supported};
use lex_data::{DataDir, Language};

/// Plays `script` (one input per line) into `lex assist` with `extra` flags and returns stdout.
fn assist(script: &str, extra: &[&str]) -> String {
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
            ("plumb", 500),
            ("sound", 400),
        ],
    );
    seed_dict(&dir, Language::English);
    let mut child = Command::new(env!("CARGO_BIN_EXE_lex"))
        .args([
            "assist",
            "-l",
            "5",
            "--lang",
            "english",
            "--color",
            "never",
            "--no-progress",
            "--data-dir",
        ])
        .arg(tmp.path())
        .args(extra)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "lex assist failed: {output:?}");
    // Prompts end in a space, which an editor hook would strip from the stored snapshots.
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_scripted_game_is_graded_turn_by_turn() {
    insta::assert_snapshot!(assist("crane\nxxxxx\n\n22222\n", &[]));
}

#[test]
fn details_show_the_full_numbers() {
    insta::assert_snapshot!(assist("crane\nxxxxx\n\n22222\n", &["--details"]));
}

#[test]
fn a_played_word_other_than_the_pick_is_graded_against_it() {
    insta::assert_snapshot!(assist("stare\nxxxxx\nplumb\n22222\n", &["--details"]));
}

#[test]
fn no_explain_has_no_grading_line() {
    let out = assist("crane\nxxxxx\n\n22222\n", &["--no-explain"]);
    assert!(!out.contains("Luck:") && !out.contains("Skill:"), "{out}");
}
