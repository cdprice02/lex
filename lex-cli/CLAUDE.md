# CLAUDE.md — lex-cli

## Purpose

CLI frontend with four subcommands: `simulate` (batch games via `lex-core`),
`assist` (interactive helper for live games), and `compare` (paired diff of two
simulation reports), and `clear` (delete a cached corpus with `--lang [-l]`, or a fetched Profile with `--profile`; exactly one). Loads corpora via `lex-data` and
monomorphizes the call chain over word lengths 3–10 at compile time.

## Module map

| Module | Responsibility |
|---|---|
| `cli.rs` | clap `Cli`/`Command`; `CommonArgs` flattened into `SimulateArgs`/`AssistArgs`; `--lang` and `-l` have no defaults: each is required unless `--profile` is given and conflicts with it (the Profile fixes both); `--profile` also conflicts with `--dictionary-length`; `--hard` and `--ultrahard` conflict with each other; `CommonArgs::dispatch_length()` picks the length to monomorphize over |
| `simulate.rs` | batch simulation loop; builds and emits the `Report` |
| `lists.rs` | `GameLists<N>` (pool, Answer list; a Corpus run answers with every pool word) and `Source` (`Corpus` or `Profile`, with the report's Corpus info and `ProfileStats`), both from `Source::load`. `Source` owns `dictionary_length()` (the Corpus size, fixed at load), `answer_noun()`, the report's provenance and assist's Guess-pool check; a new source is a new variant |
| `targets.rs` | target selection: seeded uniform sample (ChaCha8, partial Fisher-Yates) or `--targets` file |
| `report.rs` | JSON schema (`Report`), uniform and prior-weighted `Stats`, text summary |
| `compare.rs` | paired per-target comparison of two reports, joined on word |
| `assist.rs` | interactive loop; `parse_feedback` (private) auto-detects input format |
| `error.rs` | `CliError` |

## Monomorphization dispatch

`lex_data::match_word_length!(f, len, args)` expands (via `seq_macro`) to a
`match` on `len` calling `f::<N>(args)` for each `N` in 3..=10. The macro lives
in `lex-data` next to the `MIN_WORD_LENGTH`/`MAX_WORD_LENGTH` consts it mirrors
(compile-time assert keeps them in lockstep), so future frontends reuse it.
`main.rs` invokes it once per subcommand.

## Simulate loop (`simulate.rs`)

1. Load `GameLists<N>` and its `Source` with the shared args; capture the evaluation prior over the whole pool (`answer_prior` at `CANONICAL_TEMPERATURE`, whatever the solver's `--temperature`); only targets are looked up. `corpus.dictionary_length` is `Source::dictionary_length()`, the Corpus size (`DataDir::ngrams_count` on a Profile, the pool size otherwise)
2. Select targets from the Answer list, or the whole dictionary without a Profile (`--seed`, `--targets`, `-n`); `--targets` words must be in that list
3. Build one `Guesser` from `CommonArgs::strategy()` (`--hard`, `--ultrahard`, `--temperature`, `--win-weight`) and read `first_guess` from its root suggestion (reported, not replayed)
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
