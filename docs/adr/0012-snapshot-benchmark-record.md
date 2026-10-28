# Snapshot benchmark record: the committed snapshot is the base

Each Track has one committed insta snapshot in `benchmarks/snapshots/`, and a solver change is checked against it, not against a rerun of the base commit. The snapshot body holds the Track id, the run configuration, the Corpus checksum and the Profile checksums, the summary (both means, the failure rate, the histogram), the first guess and the SHA-256 of the canonical games array (compact `serde_json`). The header holds the solver-source digest (`lex_core::SOLVER_DIGEST`, a `lex-core` build script's SHA-256 over `lex-core/src` and `lex-core/Cargo.toml`, sorted by path) and the lex version; insta never compares the header, so a comment-only edit refreshes digests without touching the body, and `cargo insta test --force-update-snapshots` does it. A behavior-preserving change shows no body diff; a behavior-changing change commits its accepted snapshots in a separate `test(snapshots): ...` commit, reviewed by diff. A Corpus rebuild is one reviewed snapshot-update commit.

Each Track is a test named `<tier>::<track-id>` in `lex-cli/tests/tracks.rs`, ignored by default so `cargo test --all` and CI need no data. `-- --ignored headline` runs the headline Tier and a bare `--ignored` runs every Track. The tests read the Corpus from `LEX_DATA_DIR` (default the workspace `data/`) and run locally, never in CI.

This supersedes ADR 0009 and replaces the Oracle and the committed baselines. It amends ADR 0007: the Oracle's fast and full sets are now the headline Tier and every Tier of snapshots, and the Track table's targets are set by rule, with no calibration command.

## Considered Options

- Keeping the base-commit rerun (ADR 0009): rejected; a snapshot is the base, needs no second build and no cache keyed by commit, and a Corpus rebuild is one diff instead of a stale cache.
- Snapshots of the full report: rejected; the games array is megabytes per Track, so the body holds its digest, and `lex simulate` on both builds with `lex compare` finds which game changed.
- Putting the solver digest in the body: rejected; every edit to `lex-core` would then fail the snapshot, including edits that leave play untouched.
- Running the snapshots in CI: rejected; they need the Corpus, which ADR 0003 keeps out of the repository.
