# CLAUDE.md — lex-core

## Purpose

Entropy-based Wordle solver. Pure computation — no I/O. Depends only on `lex-data` for the `Word`/`WordSet` types.

## Module map

| Module | Responsibility |
|---|---|
| `correctness.rs` | `Correctness`, `WordCorrectness<N>`, `correct()`, `ordinal()` |
| `candidates.rs` | `Candidates<N>` (crate-private): the shrinking word set, `filtered(guess)`, entropy `suggest`/`suggest_top_k`, private `guess_entropy()` |
| `tree.rs` | `DecisionTree<N>` (crate-private): arena of nodes, one per game position, with a memoized suggestion each |
| `guesser.rs` | `Guess<N>`, `Guesser<N>`: history plus a cursor into a shared `DecisionTree` |
| `game.rs` | `GameResult<N>`, `play()`, private `log_guess()` |

All public types are re-exported from `lib.rs`: `use lex_core::{Guesser, Guess, WordCorrectness, Correctness, GameResult, play}`.

## Decision tree (`tree.rs`)

A game position is identified by its guess path: the `(guess word, feedback ordinal)` steps from the
root. For a fixed starting `WordSet` the path determines the candidate set, and `Candidates::suggest`
is deterministic, so a node computes its suggestion once and every later game reaching it reuses
it. Edges are sparse (`HashMap<(Word<N>, u16), NodeId>`: 59,049 possible patterns at N=10) and
keyed by the guess word as well as the pattern, because callers may push guesses that were not the
suggestion (`assist` does). Ordinals are `u16`; a const assertion in `correctness.rs` ties that to
`MAX_WORD_LENGTH`. If `suggest` ever gains randomness or hidden state, the cache is invalid.

`Guesser` holds the tree behind `Rc<RefCell<..>>`, so it is not `Send`; `Guesser::reset()` starts a
new game on the same tree, and `play` takes `&mut Guesser` for that reason.

## Entropy scoring (`guess_entropy` in `candidates.rs`)

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

- `guesser.rs`: the strategy abstraction lands with unconstrained guessing (`.tasks/05-strategy.md`) — immutable guess pool + shrinking candidate set weighted by an answer prior. The prior (temperature softmax) moves here from `lex-data` in the same change.
