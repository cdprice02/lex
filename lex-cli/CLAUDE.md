# CLAUDE.md — lex-cli

## Purpose

CLI frontend, a library (`lex_cli`, internal: `src/lib.rs`) plus the `lex` binary (`main.rs` parses and dispatches), with six subcommands (the sixth, `verdict`, is hidden plumbing): `simulate` (batch games via `lex-core`),
`assist` (interactive helper for live games), `compare` (paired diff of two
simulation reports), `book build` (build or refresh the opening book), and `clear` (delete a cached corpus with `--lang [-l]`, a fetched Profile with `--profile`, or just the opening books with `--books [--lang] [-l]`; a corpus or Profile takes its books with it). Loads corpora via `lex-data` and
monomorphizes the call chain over word lengths 3–21 at compile time.

## Module map

| Module | Responsibility |
|---|---|
| `cli.rs` | clap `Cli`/`Command`; `CommonArgs` flattened into `SimulateArgs`/`AssistArgs`; `--lang` and `-l` have no defaults: each is required unless `--profile` is given and conflicts with it (the Profile fixes both); `--profile` also conflicts with `--dictionary-length`; `--pool <all|hard|ultrahard>` (parsed by `GuessPool`'s `FromStr`) picks the Guess pool; `CommonArgs::dispatch_length()` picks the length to monomorphize over, `CommonArgs::config(targets)` builds the `RunConfig`; `SimulateArgs::config()` turns `-n`/`--seed`/`--targets` into `Targets` (`--targets` conflicts with `-n`; `--seed` needs `-n`), and the hidden `--track <id>` plays a Track's config and conflicts with every game, pool, strategy and target flag |
| `config.rs` | `RunConfig { game, strategy, targets }`: `Game` (`Corpus { language, length, dictionary_length }` or `Profile`) and `Targets` (`Full`, `Sample { n, seed }`, `File`). One value for what a run plays, built from flags or from a Track |
| `tracks.rs` | `TRACKS`, the `const` table of benchmark Tracks (`Track { game, strategy, tier, judged, targets }`), built by a `const fn` over `Language::VARIANTS` and the length range; `TRACK_COUNT` is computed from them, so a new language or length needs no edit. `Track::config()`, `Track::find(id)`, ids from `Display`. Tier and target rules are ADR 0007's; the evidence is `benchmarks/experiments/track-targets.md` |
| `books.rs` | Opening book files for a run: `key()` builds the `BookKey` (Corpus and Profile checksums, length, pool size, `Strategy::key_fields()`, `SOLVER_DIGEST`); `ensure()` loads the book, or on a miss builds it (progress and a stale-field message on stderr) and stores it, errors on a damaged file naming it and `lex book build --force`; `guesser()` returns a `Guesser` seeded with it unless `BookMode::Off` (`--no-book`); `build_command` is `lex book build`. A book always covers the full Answer set, whatever `-n` samples. Build time goes to stderr and `book build`'s stdout, never into the Report |
| `clear.rs` | `clear(&ClearArgs)`: `--books` removes only books; otherwise a Corpus (with its books) or a Profile (with the books whose key names it) |
| `simulate.rs` | `run(&RunConfig, &DataDir) -> Result<Report>` plays a config with its book, built on first use (`run_with(.., BookMode)` opts out), and prints nothing (it dispatches the word length itself); `simulate::<N>(&SimulateArgs)` is the command: `args.config()`, `run`, then print or write the report |
| `lists.rs` | `GameLists<N>` (pool, Answer list; a Corpus run answers with every pool word) and `Source` (`Corpus` or `Profile`, with the report's Corpus info and `ProfileStats`), both from `Source::load`. `Source` owns `dictionary_length()` (the Corpus size, fixed at load), `answer_noun()`, the report's provenance and assist's Guess-pool check; a new source is a new variant |
| `targets.rs` | target selection: seeded uniform sample (ChaCha8, partial Fisher-Yates) or `--targets` file |
| `tests/tracks.rs` | libtest-mimic harness (`harness = false`): one ignored trial per `TRACKS` entry, named `<tier>::<track-id>`, that plays `simulate::run(&track.config(), &data_dir)` (which builds a missing Opening book first, like `simulate`) and compares an insta snapshot in `benchmarks/snapshots/` (body: Track id, config, Corpus and Profile checksums, summary, first guess, games SHA-256; header `info`: `SOLVER_DIGEST`, lex version). Data comes from `LEX_DATA_DIR` (default the workspace `data/`); missing data fails with a download hint. Loop: `cargo test --release -p lex-cli --test tracks -- --ignored headline` (fast set), `-- --ignored` (full set), `cargo insta test --force-update-snapshots` to refresh header digests after a behavior-preserving change; behavior-changing PRs commit snapshots in a separate `test(snapshots): ...` commit (ADR 0012) |
| `verdict.rs` | `lex verdict`, see Plumbing |
| `report.rs` | JSON schema (`Report`), uniform and prior-weighted `Stats`, text summary |
| `compare.rs` | paired per-target comparison of two reports, joined on word |
| `assist.rs` | interactive loop; `parse_feedback` (private) auto-detects input format |
| `error.rs` | `CliError` |

## Plumbing

`lex verdict` is hidden from `--help`; CI and the benchmark loop use it, people rarely do.
`lex verdict <base-dir> <head-dir>` reads two directories of Track snapshots (the YAML header via `yaml_serde` for the solver digest, the body as JSON) and prints a markdown table and an `Outcome:` line by ADR 0007's rule; exit 0 accept, unchanged, rebaseline or new, 1 reject or a stale headline digest, 2 bad input. A Track's tier and judged flag come from `TRACKS` by id, never the snapshot. `lex verdict --readme <snap-dir> [--check] [--file PATH]` rewrites, or checks, the headline table between `<!-- tracks:start -->` and `<!-- tracks:end -->` in `benchmarks/README.md`. `verdict()` takes the expected digest as a parameter; `tests/verdict.rs` runs the binary over `tests/fixtures/verdict/` (a `base/` set and per-case head overlays, with `FIXTURE_DIGEST` replaced by the compiled digest). Methodology: `benchmarks/README.md`.

## Monomorphization dispatch

`lex_data::match_word_length!(f, len, args)` expands (via `seq_macro`) to a
`match` on `len` calling `f::<N>(args)` for each `N` in 3..=21. The macro lives
in `lex-data` next to the `MIN_WORD_LENGTH`/`MAX_WORD_LENGTH` consts it mirrors
(compile-time assert keeps them in lockstep), so future frontends reuse it.
`main.rs` invokes it once per subcommand.

## Simulate loop (`simulate.rs`)

1. Load `GameLists<N>` and its `Source` from the `RunConfig`; capture the evaluation prior over the whole pool (`answer_prior` at `CANONICAL_TEMPERATURE`, whatever the solver's `--temperature`); only targets are looked up. `corpus.dictionary_length` is `Source::dictionary_length()`, the Corpus size (`DataDir::ngrams_count` on a Profile, the pool size otherwise)
2. Select targets from the Answer list, or the whole dictionary without a Profile (`Targets`: all, a seeded sample, or a file); file words must be in that list
3. Build one `Guesser` from the config's `Strategy` (`--pool`, `--temperature`, `--win-weight`), seeded with the Opening book (`books::guesser`; `--no-book` skips it) and read `first_guess` from its root suggestion (reported, not replayed)
4. For each game: call `play(word, &mut guesser, Vec::new())`; games share the guesser's decision tree, so each position is scored once
5. Emit the report: summary to stdout (stderr when `-o -` sends JSON to stdout), JSON to `-o PATH`

Sharing the tree is only valid while `Candidates::suggest` is deterministic (see `lex-core/CLAUDE.md`).

## Assist loop (`assist.rs`)

With `--profile`, a played word outside the Profile's Guess pool is rejected ("is not in the `<profile>` guess pool"); without one any `Word<N>` is accepted.

Per turn: show `suggest_top_k(--suggestions)` with entropy bits (non-candidate probes are marked) → prompt for
the word actually played (empty input = top suggestion; may be any `Word<N>`,
not necessarily suggested or in the corpus) → prompt for feedback →
`push_guess`. Exits on all-correct feedback, candidate exhaustion (target not
in corpus / inconsistent entry), or EOF.

Feedback formats are auto-detected from the first symbol and must not be mixed
within one entry: letters `g`/`y`/`x` (gray aliases `b` `.` `-`), digits
`2`/`1`/`0` (matches the `Correctness` encoding), emoji including dark-mode ⬛
and high-contrast 🟧/🟦 variants.

## Error handling

CLI argument validation errors use `CliError` (defined in `error.rs`, derived with `thiserror`). Runtime errors from `lex-data` and `lex-core` propagate as `anyhow::Result`.

## Reports

`simulate -o report.json` writes schema v1: `run` (seed, timestamp, first guess, `strategy` with a `mode` of `unconstrained`, `hard` or `ultrahard`; the older `hard_mode` bool reads as ultrahard or unconstrained; reports without a `strategy` read as ultrahard, T=5, win weight 0), `corpus` (ngrams sha256, dict provenance), `summary` (histogram, mean, failure rate under `uniform` and `prior` weighting), and per-target `games` with the full guess sequence and prior, and `profile` (`serde(default)`, absent for a Corpus run: name, list checksums, counts, Unseen-word floor; `summary.uniform` is its headline). Failure means more than 6 guesses; the solver still plays to completion so the true count is recorded. The prior weighting renormalizes over the targets played: exact for a full-dictionary run, a self-normalized estimate for a sample. `lex compare baseline.json candidate.json` pairs targets by word and warns when corpus checksums differ; it prints the strategies when they differ, and hard-errors when the reports played different Profiles or only one played a Profile.
