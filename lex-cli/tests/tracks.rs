//! One snapshot test per Track (ADR 0012). Each plays its Track in-process on the local Corpus
//! and compares the result with the committed snapshot in `benchmarks/snapshots/`. The trials
//! are ignored by default, so `cargo test` needs no data; `-- --ignored <tier>` runs a Tier and
//! a bare `--ignored` runs every Track.

use std::path::PathBuf;

use insta::{Settings, assert_json_snapshot};
use lex_cli::config::Game;
use lex_cli::report::Report;
use lex_cli::simulate::run;
use lex_cli::tracks::{TRACKS, Track, TrackTargets};
use lex_data::DataDir;
use libtest_mimic::{Arguments, Trial};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// `LEX_DATA_DIR`, else the workspace's `data/`.
fn data_dir() -> PathBuf {
    std::env::var_os("LEX_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("data"))
}

/// Fails with a download hint when the cache the Track reads is absent: a run would otherwise
/// start downloading every ngrams shard of the language.
fn require_data(track: &Track, dir: &DataDir, root: &std::path::Path) -> Result<(), String> {
    let (language, length) = match track.game {
        Game::Corpus {
            language, length, ..
        } => (language, length),
        Game::Profile(profile) => {
            let spec = profile.spec();
            (spec.language, spec.word_length)
        }
    };
    let missing = [dir.ngrams_path(language, length), dir.dict_path(language)]
        .into_iter()
        .find(|path| !path.exists());
    match missing {
        None => Ok(()),
        Some(path) => Err(format!(
            "{} is missing, so Track {track} has no data in {}. Download it once with \
             `cargo run --release -- simulate --track {track} --data-dir {}`, or point \
             LEX_DATA_DIR at a populated data directory.",
            path.display(),
            root.display(),
            root.display()
        )),
    }
}

fn config_json(track: &Track, report: &Report) -> Value {
    let game = match track.game {
        Game::Corpus {
            language,
            length,
            dictionary_length,
        } => json!({
            "kind": "corpus",
            "language": language.to_string(),
            "length": length,
            "dictionary_length": dictionary_length,
        }),
        Game::Profile(profile) => json!({ "kind": "profile", "profile": profile.to_string() }),
    };
    let targets = match track.targets {
        TrackTargets::Full => json!({ "kind": "full" }),
        TrackTargets::Sample { n, seed } => json!({ "kind": "sample", "n": n, "seed": seed }),
    };
    json!({ "game": game, "strategy": report.run.strategy, "targets": targets })
}

fn play(track: &Track) -> Result<(), String> {
    let root = data_dir();
    let data = DataDir::new(&root);
    require_data(track, &data, &root)?;
    let report = run(&track.config(), &data).map_err(|e| format!("{e:#}"))?;

    let games = serde_json::to_vec(&report.games).map_err(|e| e.to_string())?;
    let games_sha256: String = Sha256::digest(&games)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let profile = report.profile.as_ref().map(|p| {
        json!({
            "name": p.name,
            "answers_sha256": p.answers_sha256,
            "guess_only_sha256": p.guess_only_sha256,
        })
    });
    let body = json!({
        "track": track.to_string(),
        "config": config_json(track, &report),
        "corpus_sha256": report.corpus.ngrams_sha256,
        "profile": profile,
        "summary": report.summary,
        "first_guess": report.run.first_guess,
        "games_sha256": games_sha256,
    });

    // The digest and version describe the build, not the play, so they are the snapshot's
    // header and never decide a match.
    let mut settings = Settings::new();
    settings.set_snapshot_path(workspace().join("benchmarks/snapshots"));
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings.set_info(&json!({
        "solver_digest": lex_core::SOLVER_DIGEST,
        "lex_version": report.run.lex_version,
    }));
    let name = track.to_string();
    settings.bind(|| assert_json_snapshot!(name, body));
    Ok(())
}

fn main() {
    let args = Arguments::from_args();
    let trials = TRACKS
        .iter()
        .map(|track| {
            let name = format!("{}::{track}", track.tier.name());
            let track = *track;
            Trial::test(name, move || play(&track).map_err(Into::into)).with_ignored_flag(true)
        })
        .collect();
    libtest_mimic::run(&args, trials).exit();
}
