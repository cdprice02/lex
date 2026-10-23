# CLAUDE.md — lex-cli

## Purpose

CLI frontend with four subcommands: `simulate` (batch games via `lex-core`),
`assist` (interactive helper for live games), and `compare` (paired diff of two
simulation reports), and `clear` (delete cached corpora so they rebuild). Loads corpora via `lex-data` and
monomorphizes the call chain over word lengths 3–10 at compile time.

## Module map

| Module | Responsibility |
|---|---|
| `cli.rs` | clap `Cli`/`Command`; `CommonArgs` flattened into `SimulateArgs`/`AssistArgs` |
| `simulate.rs` | batch simulation loop; builds and emits the `Report` |
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

1. Load `WordSet<N>` via `blocking::DataDir` with the shared args; capture the answer prior of every word over the full dictionary
2. Select targets (`--seed`, `--targets`, `-n`; default is the whole dictionary)
3. Build one `Guesser` and read `first_guess` from its root suggestion (reported, not replayed)
4. For each game: call `play(word, &mut guesser, Vec::new())`; games share the guesser's decision tree, so each position is scored once
5. Emit the report: summary to stdout (stderr when `-o -` sends JSON to stdout), JSON to `-o PATH`

Sharing the tree is only valid while `Candidates::suggest` is deterministic (see `lex-core/CLAUDE.md`).

## Assist loop (`assist.rs`)

Per turn: show `suggest_top_k(--suggestions)` with entropy bits → prompt for
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

`simulate -o report.json` writes schema v1: `run` (seed, timestamp, first guess), `corpus` (ngrams sha256, dict provenance), `summary` (histogram, mean, failure rate under `uniform` and `prior` weighting), and per-target `games` with the full guess sequence and prior. Failure means more than 6 guesses; the solver still plays to completion so the true count is recorded. The prior weighting renormalizes over the targets played: exact for a full-dictionary run, a self-normalized estimate for a sample. `lex compare baseline.json candidate.json` pairs targets by word and warns when corpus checksums differ.
