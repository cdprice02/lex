# CLAUDE.md — lex-cli

## Purpose

CLI frontend with two subcommands: `simulate` (batch games via `lex-core`) and
`assist` (interactive helper for live games). Loads corpora via `lex-data` and
monomorphizes the call chain over word lengths 3–10 at compile time.

## Module map

| Module | Responsibility |
|---|---|
| `cli.rs` | clap `Cli`/`Command`; `CommonArgs` flattened into `SimulateArgs`/`AssistArgs` |
| `simulate.rs` | batch simulation loop |
| `assist.rs` | interactive loop; `parse_feedback` (private) auto-detects input format |
| `error.rs` | `CliError` |

## Monomorphization dispatch

`lex_data::match_word_length!(f, len, args)` expands (via `seq_macro`) to a
`match` on `len` calling `f::<N>(args)` for each `N` in 3..=10. The macro lives
in `lex-data` next to the `MIN_WORD_LENGTH`/`MAX_WORD_LENGTH` consts it mirrors
(compile-time assert keeps them in lockstep), so future frontends reuse it.
`main.rs` invokes it once per subcommand.

## Simulate loop (`simulate.rs`)

1. Load `WordSet<N>` via `blocking::DataDir` with the shared args
2. Pre-compute `first_guess` once — `Guesser` is deterministic, so the same word set always produces the same first suggestion
3. For each game: construct a one-element history (`Guess::new(first_guess, WordCorrectness::correct(word, first_guess))`) and call `play(word, &word_set, history)`
4. Aggregate `GameResult::num_guesses()` across all games

The first-guess cache is only valid as long as `Guesser::suggest()` is deterministic. The comment in the code flags this assumption explicitly.

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
