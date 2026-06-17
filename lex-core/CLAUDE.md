# CLAUDE.md — lex-core

## Purpose

Entropy-based Wordle solver. Pure computation — no I/O. Depends only on `lex-data` for the `Word`/`WordSet` types.

## Module map

| Module | Responsibility |
|---|---|
| `correctness.rs` | `Correctness`, `WordCorrectness<N>`, `correct()`, `ordinal()` |
| `guesser.rs` | `Guess<N>`, `Guesser<N>`, private `guess_entropy()` |
| `game.rs` | `GameResult<N>`, `play()`, private `log_guess()` |

All public types are re-exported from `lib.rs`: `use lex_core::{Guesser, Guess, WordCorrectness, Correctness, GameResult, play}`.

## Entropy scoring (`guess_entropy` in `guesser.rs`)

For each candidate guess, iterates all remaining words and accumulates probability mass per `WordCorrectness` pattern (indexed by `ordinal()`). Returns negative entropy — higher is more informative.

Uses a thread-local `PATTERN_PROBS: Vec<f64>` of length `WordCorrectness::<N>::COUNT` (= 3^N). The vec is allocated once per thread and `.fill(0.0)` reset between calls. Marked `#[optimize(speed)]`.

## Correctness algorithm (`correct()` in `correctness.rs`)

Two-pass algorithm with a thread-local `CHAR_COUNTS: Box<[u8; 0x10000]>` BMP buffer:

1. **Pass 1** — mark exact matches (`Correct`); for non-matching positions, increment `counts[word_char]`
2. **Pass 2** — for non-Correct positions, if `counts[guess_char] > 0` mark `Misplaced` and decrement

Buffer reset is selective: only positions written in pass 1 are zeroed, avoiding a full 65 KB memset per call. The `debug_assert` guards against chars above U+FFFF.

## `ordinal()` encoding

`WordCorrectness` is encoded as a base-3 number (position 0 = most significant digit). `Absent = 0, Misplaced = 1, Correct = 2`. All-absent = 0; all-correct = 3^N − 1. This gives a compact index into the pattern probability array.

## TODO

- `guesser.rs`: when a second strategy is needed, extract a `GuesserStrategy` trait — current `push_guess` + `suggest` signatures are already trait-compatible.
