# Benchmarks

How solver changes are measured, and where the current numbers stand. The objective and the
acceptance rule for a change are in `docs/adr/0007-benchmark-objective.md`.

## Layout

| Path | Holds |
|---|---|
| `README.md` | Methodology and the current results (the table is generated) |
| `snapshots/*.snap` | One committed insta snapshot per Track: the record a change is checked against |
| `experiments/<topic>.md` | The evidence and verdict behind each solver or performance choice |
| `scripts/` | Harness scripts (`sweep.sh`: temperature by win-weight grid) |

## Current results

Full-dictionary runs of the headline Tier, default strategy unless the Track says otherwise
(unconstrained, T=5, win weight 1, endgame off). The table is generated from the committed
snapshots by `lex verdict --readme benchmarks/snapshots`, and CI fails when it is stale.

<!-- tracks:start -->
| Track | Targets | Uniform mean | Prior-weighted mean | Failure rate (>6) | Corpus |
|---|---|---|---|---|---|
| english-5 | full | 4.3478 | 4.1801 | 0.38% | `ed5ad0f39c90` |
| french-5 | full | 4.2329 | 4.0791 | 0.38% | `d00df27d4d4c` |
| german-5 | full | 4.0427 | 3.9341 | 0.02% | `c74b64153cb4` |
| spanish-5 | full | 4.2954 | 4.1255 | 1.16% | `b171872e1550` |
| italian-5 | full | 3.7915 | 3.6845 | 0.02% | `4bc3cf41a5c3` |
| russian-5 | full | 4.5951 | 4.4719 | 0.93% | `0df2e0461d82` |
| wordle | full | 3.4721 | 3.3935 | 0.00% | `ed5ad0f39c90` |
| wordle-all | full | 4.1072 | 3.9193 | 0.37% | `ed5ad0f39c90` |
| wordle-hard | full | 3.5806 | 3.4663 | 0.48% | `ed5ad0f39c90` |
| wordle-ultrahard | full | 3.5823 | 3.4658 | 0.48% | `ed5ad0f39c90` |
| wordle-original | full | 3.5832 | 3.4897 | 0.22% | `ed5ad0f39c90` |
<!-- tracks:end -->

## Hard and ultrahard

Hard mode is the official Wordle rule, as in Alex Selby's `-H` (`okhard` in
[selby_wordle](https://github.com/biorhythmist/selby_wordle)): greens stay in place, and every green
or yellow letter of each earlier guess reappears with at least that multiplicity. His published
optimum for it is 3.5084 mean guesses on the 2,315 answers. Ultrahard (his `-U`) plays only words
consistent with every feedback so far; no published optimum was found for it. Both are reported,
never judged (`docs/adr/0007-benchmark-objective.md`).

| Run | Uniform mean | Failure rate (>6) | Peak RSS | Selby |
|---|---|---|---|---|
| `--profile wordle --pool hard` | 3.5806 | 0.48% | 8 MB | 3.5084 (`-H`) |
| `--profile wordle --pool ultrahard` | 3.5823 | 0.48% | 6 MB | none published |

Full 2,315-answer runs on an idle machine, well under the 1 GB cap; timings and the rest of the
figures are in the [measurement comment](https://github.com/cdprice02/lex/pull/68#issuecomment-6452539442).

## Experiments

| Topic | Verdict |
|---|---|
| [Strategy](experiments/strategy.md) | Unconstrained guessing, T=5, win weight 1 |
| [Micro-optimization](experiments/micro-opt.md) | Generic ordinal and selective reset adopted; byte words, bounds-check removal rejected |
| [Endgame](experiments/endgame.md) | Exact search below a threshold, off by default |
| [Partition dedupe](experiments/dedupe.md) | Rejected: slower, and over the memory cap on a full run |
| [Opening book floor](experiments/book-floor.md) | 40 M pattern evaluations (about 1 s single-core); the Wordle Profile books nothing |
| [Track targets](experiments/track-targets.md) | Full for headline and length-3 breadth, 500 sampled for other breadth, 100 for smoke; the calibration behind the rule |

## Tracks

The Tracks are one `const` table in `lex-cli/src/tracks.rs`, built from the supported languages and
word lengths, the Profiles and the guess pools; adding a language or raising `MAX_WORD_LENGTH` adds
its Tracks with no table edit. A Track's id is `{language}-{length}` (`english-5`) or the Profile's
(`wordle`, `wordle-all`, `wordle-hard`, `wordle-ultrahard`, `wordle-original`), and `lex simulate --track <id>` (hidden
from `--help`) plays it.

| Tier | Tracks | Targets | Judged |
|---|---|---|---|
| headline | every language at length 5; the four Profile Tracks | full | `wordle` only |
| breadth | every language at 3, 8, 13 and 21 (those Supported) | full at length 3, else 500 sampled | yes |
| smoke | every language at the other Supported lengths | 100 sampled | no |

Sampled Tracks draw with seed 0. The evidence for the targets is in
[experiments/track-targets.md](experiments/track-targets.md).

## Full-dictionary runs

Omit `-n` to play every word: exact expected performance with no sampling variance. The decision
tree makes this affordable (English-5 in about a minute on one core, 41 MB peak RSS; timings are in the experiments). Use it for the
headline numbers of a change.

## Snapshots

Each Track has one committed insta snapshot in `snapshots/`, named by the Track id, and a change is
checked against it (`docs/adr/0012-snapshot-benchmark-record.md`). The body holds the Track id, the
run configuration, the Corpus and Profile checksums, the summary (both means, the failure rate, the
histogram), the first guess and the SHA-256 of the canonical games array. The header holds the
solver digest (`lex_core::SOLVER_DIGEST`) and the lex version, which insta never compares.

The tests are `lex-cli/tests/tracks.rs`, one per Track, named `<tier>::<track-id>` and ignored by
default, so `cargo test --all` needs no data. They read the Corpus from `LEX_DATA_DIR` (default the
workspace `data/`) and fail with a download hint when it is missing. They run locally, never in CI.
Run them with `--release`.

```bash
cargo test --release -p lex-cli --test tracks -- --ignored headline   # fast set: the headline Tier
cargo test --release -p lex-cli --test tracks -- --ignored            # full set: every Track
cargo insta test --release -p lex-cli --test tracks --force-update-snapshots -- --ignored headline
```

- A behavior-preserving change (a speed change, a refactor) must show no body diff. Because it edits
  `lex-core/src`, the digest in the headers is stale: refresh it with the third command, so the
  snapshot diff is only header digest lines. Breadth and smoke headers may lag.
- A behavior-changing change commits its accepted snapshots in a separate `test(snapshots): ...`
  commit, reviewed by diff; a Corpus rebuild is likewise one snapshot-update commit.
- The full set when the change touches word-length-dependent code (pattern ordinals, counting, the
  length limit), and once at the end of a map.
- To find which game changed, run `lex simulate` on both builds and `lex compare` the reports.
- Each Track runs on its own test thread. On a small machine, pass `--test-threads N` after `--`.
- `cargo-insta` comes with the Nix devshell; with rustup, `cargo install cargo-insta`.
- `data/oracle/` held the old Oracle's cached base reports and can be removed.

## Micro-benchmarks

Per-function timings use divan, behind each library crate's `bench` feature:

```bash
cargo bench -p lex-core --features bench
cargo bench -p lex-data --features bench
cargo bench -p lex-core --features bench -- ordinal_count     # substring of the bench path
cargo bench -p lex-core --features bench -- --exact correctness::benches::ordinal_count
cargo test --workspace --all-features --benches               # CI: each bench runs once
```

They diagnose where time goes; there is no saved baseline, and they never judge a change.

## Speed bar

A speed change is judged on end-to-end CPU time with hyperfine, base against head, each built
`--release` in its own checkout. One round is one invocation per binary with `--runs 1`, and the
order of base and head alternates between rounds so thermal drift and cache warmth do not favour
one side. Time warm: build each binary's book first (`lex book build ...` with the same flags, which
prints its node count, size and build time), so a round measures play and the build time is reported
beside it. Run at least 3 rounds:

```bash
# once per binary: base/target/release/lex book build -l 5 --lang english
# round 1: base first, round 2: head first, round 3: base first, ...
hyperfine -N --runs 1 --export-json /tmp/round1.json \
  'base/target/release/lex simulate -l 5 --lang english --seed 0' \
  'head/target/release/lex simulate -l 5 --lang english --seed 0'
```

Summarize each command's mean and spread across the round files:

```bash
jq -s -r '[.[].results[]] | group_by(.command)[] |
  (map(.mean)) as $m | ($m | add / length) as $a |
  "\(.[0].command)\tmean \($a)s\tmin \($m | min)s\tmax \($m | max)s"' /tmp/round*.json
```

Check for stray CPU-heavy processes first, and rerun any figure taken under contention. To see
where the time goes, profile with `samply record -- ./target/release/lex simulate ...` (the
`samply` Cargo profile keeps debug info).

## Verdict

`lex verdict` (hidden from `--help`) applies ADR 0007's rule to two snapshot directories. CI runs it
on every pull request (`.github/workflows/verdict.yml`) with the base branch's `benchmarks/snapshots`
and the head's, and writes the table to the job summary. To run it locally, extract the base
snapshots and compare:

```bash
mkdir -p /tmp/base && git archive origin/main benchmarks/snapshots | tar -x -C /tmp/base
lex verdict /tmp/base/benchmarks/snapshots benchmarks/snapshots
```

It prints a table per Track (tier, judged, base and head score, delta, status) and an `Outcome:` line.
The score is the uniform mean on a Profile Track and the prior-weighted mean on a Corpus Track; a
Track's tier and whether it is judged come from the Track table, not the snapshot.

| Outcome | Meaning | Exit |
|---|---|---|
| `accept` | the average judged headline delta is negative, no headline Track is worse by more than 0.005, no breadth Track by more than 0.02; within 0.001 of zero the average failure-rate delta must also be negative | 0 |
| `unchanged` | every judged delta is zero | 0 |
| `rebaseline` | every judged Track changed Corpus or Profile checksum, so none can be compared | 0 |
| `new` | no judged Track exists on both sides (a base with no snapshots) | 0 |
| `reject` | a regression limit or the average rule failed | 1 |
| `stale` | a headline snapshot's solver digest differs from the `lex` binary's | 1 |
| bad input | a missing head directory, an unreadable snapshot | 2 |

Smoke, reported, new and missing Tracks are listed and never judged. A Track whose Corpus or Profile
checksum changed is `corpus changed` and left out. A breadth or smoke snapshot with an old digest
is marked `(stale)` and still compared. A behavior-changing PR therefore commits the snapshots of
the headline Tier, run on its own source; a behavior-preserving one refreshes the digests (see
Snapshots). `lex verdict --readme benchmarks/snapshots` rewrites the table above from the head
snapshots, and `--check` fails when it is out of date.

To see which games changed rather than the Track scores, run `lex simulate -o` on both builds and
`lex compare` the reports; targets are paired by word and it warns when the Corpus checksums differ.

## Provenance

KAIKKI Wiktionary extracts are rolling. A report's `corpus.ngrams_sha256` is the Corpus checksum
from the cache header (SHA-256 of the records), identifying the exact word list and frequencies it
ran against; `corpus.dict` records the Wiktionary download. A rebuilt Corpus will usually have a
different checksum, so the snapshots must be regenerated (and the change noted) when the Corpus is
rebuilt, not mixed with reports from an older one.
