# CLAUDE.md — lex-cli

## Purpose

Simulation CLI. Loads a word corpus via `lex-data`, then runs batch Wordle games using `lex-core`. Monomorphizes the entire call chain over word lengths 3–10 at compile time.

## Monomorphization dispatch

`configure_word_length_bounds!(3, 10)` (defined in `simulate.rs`, invoked from `main.rs`) generates two things:

1. A compile-time assert that the literals match `lex_data::MIN_WORD_LENGTH` / `MAX_WORD_LENGTH`
2. The `match_word_length_run!($args)` macro, which expands via `seq_macro` to a `match` on `args.word_length` calling `simulate::<N>($args)` for each N in 3..=10

This monomorphizes `Guesser`, `WordSet`, `Word`, and `WordCorrectness` for every supported length, enabling const-generic optimizations throughout.

## Simulate loop (`simulate.rs`)

1. Load `WordSet<N>` via `blocking::DataDir::new(&args.data_dir).load::<N>(args.lang, args.dictionary_length)`
2. Pre-compute `first_guess` once — `Guesser` is deterministic, so the same word set always produces the same first suggestion
3. For each game: construct a one-element history (`Guess::new(first_guess, WordCorrectness::correct(word, first_guess))`) and call `play(word, &word_set, history)`
4. Aggregate `GameResult::num_guesses()` across all games

The first-guess cache is only valid as long as `Guesser::suggest()` is deterministic. The comment in the code flags this assumption explicitly.

## Error handling

CLI argument validation errors use `CliError` (defined in `error.rs`, derived with `thiserror`). Runtime errors from `lex-data` and `lex-core` propagate as `anyhow::Result`.
