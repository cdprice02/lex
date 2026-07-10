# 01 — Assist mode + subcommand split

Depends on: 00 (preferably; nothing technically)
Unblocks: daily-Wordle dogfooding; manual test harness for all later phases

## Why

We want to play the real Wordle of the day with the solver's help while the
rest of the roadmap is in progress. An interactive assist mode changes no
solver behavior — it is `Guesser` plus a feedback-entry loop — so it does not
need the measurement gate (task 02), and it doubles as a manual test harness.

## Current state

- `lex-cli/src/cli.rs` is a single flat clap `Args` struct; a TODO there
  points at this brief.
- The monomorphization dispatch (`configure_word_length_bounds!` /
  `match_word_length_run!`) lives in `lex-cli/src/simulate.rs` and is invoked
  from `main.rs`. Assist needs the same dispatch over word length.
- `Guesser::with_history` / `push_guess` / `suggest` (lex-core) already
  support incremental interactive use.

## Requirements

- Split into clap subcommands: `simulate` (current behavior) and `assist`.
  Shared args (`--word-length`, `--lang`, `--data-dir`, `--dictionary-length`)
  stay available to both.
- Assist loop: show the suggested guess → the user enters the guess they
  *actually played* plus the feedback pattern Wordle returned → `push_guess`
  → repeat until solved or candidates are exhausted.
  - The played guess may differ from the suggestion (the real Wordle may
    reject words our corpus contains, and vice versa). `Guess::new` accepts
    any `Word<N>`, so this already works — do not assume the suggestion was
    played.
  - Validate input length and pattern characters; re-prompt on bad input.
- Generalize or relocate the dispatch macro so both subcommands (and future
  frontends) share it rather than copying it.
- Handle candidate exhaustion gracefully (clear message; the daily word may
  simply not be in our corpus).

## Open questions

- Feedback input format: letters (`gyx`/`gy.`), digits (`210`), or emoji
  paste — pick one primary format, propose to the user.
- Whether `lex` with no subcommand keeps meaning `simulate` (back-compat) or
  requires an explicit subcommand.
- Show only the top suggestion, or top-k with entropy scores (`suggest`
  currently returns only the best; extending it is allowed but keep the
  change minimal).
- Undo/correction support for typo'd feedback — nice to have, not required.

## Out of scope

- A full TUI. Line-based interaction first; `lex-tui` (ratatui, separate
  crate — see the note at the top of `lex-cli/src/simulate.rs`) is a
  follow-up once this loop is proven.
- Any strategy change. Ideas discovered while dogfooding wait for task 02's
  baselines before landing as behavior changes.
- A `dict` subcommand (listed in ROADMAP "Unscheduled"; add the subcommand
  scaffolding in a way that doesn't preclude it).

## Done when

Today's real Wordle can be solved interactively end-to-end, including a game
where the user plays a word different from the suggestion.
