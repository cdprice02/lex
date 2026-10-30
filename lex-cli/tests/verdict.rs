//! `lex verdict` end to end: the binary judges fixture snapshot sets. A case directory holds the
//! head snapshots that differ from `base/`; the head is the base with the case laid over it. The
//! fixtures carry the placeholder `FIXTURE_DIGEST`, replaced here by the compiled solver digest,
//! so a lex-core edit never invalidates them.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const PLACEHOLDER: &str = "FIXTURE_DIGEST";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/verdict")
}

fn copy_over(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace(PLACEHOLDER, lex_core::SOLVER_DIGEST);
        fs::write(to.join(path.file_name().unwrap()), text).unwrap();
    }
}

struct Sets {
    _tmp: TempDir,
    base: PathBuf,
    head: PathBuf,
}

/// Base fixtures, and a head of the base with `case` laid over it.
fn sets(case: Option<&str>) -> Sets {
    let tmp = TempDir::new().unwrap();
    let (base, head) = (tmp.path().join("base"), tmp.path().join("head"));
    copy_over(&fixtures().join("base"), &base);
    copy_over(&fixtures().join("base"), &head);
    if let Some(case) = case {
        copy_over(&fixtures().join(case), &head);
    }
    Sets {
        _tmp: tmp,
        base,
        head,
    }
}

struct Run {
    code: i32,
    stdout: String,
}

impl Run {
    fn outcome(&self) -> &str {
        self.stdout
            .lines()
            .find_map(|l| l.strip_prefix("Outcome: "))
            .unwrap_or("")
    }

    fn row(&self, id: &str) -> &str {
        let prefix = format!("| {id} |");
        self.stdout
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| panic!("no row for {id} in:\n{}", self.stdout))
    }
}

fn lex(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lex"))
        .arg("verdict")
        .args(args)
        .output()
        .unwrap()
}

fn judge(base: &Path, head: &Path) -> Run {
    let out = lex(&[base.as_os_str(), head.as_os_str()]);
    Run {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
    }
}

fn case(name: &str) -> Run {
    let s = sets(Some(name));
    judge(&s.base, &s.head)
}

#[test]
fn a_lower_headline_mean_is_accepted() {
    let run = case("accept");
    assert_eq!((run.outcome(), run.code), ("accept", 0));
    assert!(run.row("english-5").contains("better"));
}

#[test]
fn identical_sets_are_unchanged() {
    let s = sets(None);
    let run = judge(&s.base, &s.head);
    assert_eq!((run.outcome(), run.code), ("unchanged", 0));
}

#[test]
fn a_headline_regression_is_rejected_even_when_the_average_falls() {
    let run = case("headline-regression");
    assert_eq!((run.outcome(), run.code), ("reject", 1));
    assert!(run.row("english-5").contains("worse"));
}

#[test]
fn a_breadth_regression_is_rejected() {
    let run = case("breadth-regression");
    assert_eq!((run.outcome(), run.code), ("reject", 1));
    assert!(run.row("english-3").contains("worse"));
}

#[test]
fn inside_the_tie_band_a_lower_failure_rate_accepts() {
    let run = case("tiebreak-accept");
    assert_eq!((run.outcome(), run.code), ("accept", 0));
}

#[test]
fn inside_the_tie_band_a_higher_failure_rate_rejects() {
    let run = case("tiebreak-reject");
    assert_eq!((run.outcome(), run.code), ("reject", 1));
}

#[test]
fn a_changed_corpus_is_excluded_from_the_judgement() {
    let run = case("corpus-changed");
    assert_eq!((run.outcome(), run.code), ("unchanged", 0));
    assert!(run.row("english-5").contains("corpus changed"));
}

#[test]
fn when_every_judged_track_changed_corpus_the_outcome_is_rebaseline() {
    let run = case("rebaseline");
    assert_eq!((run.outcome(), run.code), ("rebaseline", 0));
    assert!(run.row("wordle").contains("corpus changed"));
}

#[test]
fn a_stale_headline_digest_fails() {
    let run = case("stale-headline");
    assert_eq!((run.outcome(), run.code), ("stale", 1));
    assert!(run.row("english-5").contains("stale"));
}

#[test]
fn a_stale_breadth_digest_is_only_shown() {
    let run = case("stale-breadth");
    assert_eq!((run.outcome(), run.code), ("unchanged", 0));
    assert!(run.row("english-3").contains("(stale)"));
}

#[test]
fn smoke_and_reported_tracks_are_listed_not_judged() {
    let run = case("not-judged");
    assert_eq!((run.outcome(), run.code), ("unchanged", 0));
    assert!(run.row("english-6").contains("not judged"));
    assert!(run.row("wordle-hard").contains("not judged"));
}

#[test]
fn a_missing_base_counts_every_track_as_new() {
    let s = sets(None);
    let run = judge(&s.base.join("absent"), &s.head);
    assert_eq!((run.outcome(), run.code), ("new", 0));
    assert!(run.row("english-5").contains("new"));
}

#[test]
fn a_missing_head_is_bad_input() {
    let s = sets(None);
    let run = judge(&s.base, &s.base.join("absent"));
    assert_eq!(run.code, 2);
}

fn readme_with_block(block: &str) -> String {
    format!("# Benchmarks\n\n<!-- tracks:start -->\n{block}<!-- tracks:end -->\n\nafter\n")
}

fn readme(snaps: &Path, file: &Path, check: bool) -> i32 {
    let mut args = vec![
        "--readme".as_ref(),
        snaps.as_os_str(),
        "--file".as_ref(),
        file.as_os_str(),
    ];
    if check {
        args.push("--check".as_ref());
    }
    lex(&args).status.code().unwrap()
}

#[test]
fn a_stale_readme_fails_the_check_and_is_rewritten() {
    let s = sets(None);
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("README.md");
    fs::write(&file, readme_with_block("old table\n")).unwrap();

    assert_eq!(readme(&s.head, &file, true), 1);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        readme_with_block("old table\n")
    );

    assert_eq!(readme(&s.head, &file, false), 0);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("| english-5 | full | 4.5000 | 4.0000 | 2.00% | `aaaa00000000` |"));
    assert!(text.contains("| wordle-hard |"));
    assert!(
        !text.contains("english-3"),
        "breadth Tracks are not in the table"
    );
    assert!(text.ends_with("<!-- tracks:end -->\n\nafter\n"));

    assert_eq!(readme(&s.head, &file, true), 0);
}

#[test]
fn a_readme_without_markers_is_bad_input() {
    let s = sets(None);
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("README.md");
    fs::write(&file, "no markers\n").unwrap();
    assert_eq!(readme(&s.head, &file, true), 2);
}

#[test]
fn the_committed_readme_matches_the_committed_snapshots() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let file = root.join("benchmarks/README.md");
    assert_eq!(readme(&root.join("benchmarks/snapshots"), &file, true), 0);
}
