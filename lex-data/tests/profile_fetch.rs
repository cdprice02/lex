//! Real fetch of the pinned Wordle lists. Run with:
//! `LEX_CORPUS_DIR=<data dir holding ngrams/eng/5.bin> cargo test -p lex-data --test profile_fetch -- --ignored --nocapture`
//! The Corpus is linked read-only into a temp dir; Profile files are written only there.

#![cfg(unix)]

use lex_data::{DataDir, Profile};

fn data_dir_with_corpus() -> (tempfile::TempDir, DataDir) {
    let corpus =
        std::env::var("LEX_CORPUS_DIR").expect("set LEX_CORPUS_DIR to a data dir with eng/5");
    let tmp = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        std::path::Path::new(&corpus).join("ngrams"),
        tmp.path().join("ngrams"),
    )
    .unwrap();
    let dir = DataDir::new(tmp.path());
    (tmp, dir)
}

#[test]
#[ignore = "fetches from the network"]
fn real_lists_match_the_compiled_in_checksums() {
    let (_tmp, dir) = data_dir_with_corpus();

    let wordle = dir.load_profile::<5>(Profile::Wordle).unwrap();
    println!("{:#?}", wordle.stats);
    assert_eq!(wordle.answers.len(), 2315);
    assert_eq!(wordle.pool.len(), 12972);
    assert_eq!(
        wordle.stats.answers_sha256,
        "f40e47975cf360ff9febe7d49c8e285054d8fa3fb9644cea575922060951d6ee"
    );

    let all = dir.load_profile::<5>(Profile::WordleAll).unwrap();
    println!("{:#?}", all.stats);
    assert_eq!(all.answers.len(), 12972);
    assert_eq!(all.pool.len(), 12972);
    assert_eq!(
        all.stats.answers_sha256,
        "feacc549249b5dcb43e2799fbbcda14ebc33d55904fb15d043779104e60c1579"
    );

    // a second load is served from disk and re-verified
    dir.load_profile::<5>(Profile::Wordle).unwrap();
    dir.clear_profile(Profile::Wordle).unwrap();
    assert!(!dir.profile_dir(Profile::Wordle).exists());
    assert!(dir.profile_dir(Profile::WordleAll).exists());
}
