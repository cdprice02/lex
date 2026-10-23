# CLAUDE.md — lex-core

## Purpose

Entropy-based Wordle solver. Pure computation — no I/O. Depends only on `lex-data` for the `Word`/`WordSet` types.

## Module map

| Module | Responsibility |
|---|---|
| `correctness.rs` | `Correctness`, `WordCorrectness<N>`, `correct()`, `ordinal()`, crate-private `pattern_ordinal()` |
| `strategy.rs` | `Strategy` (pool policy, temperature, win weight), `GuessPool`, `answer_prior()`, crate-private `Pool<N>` (immutable guess pool with log-frequencies and the renormalized prior) |
| `candidates.rs` | `Candidates<N>` (crate-private): the shrinking candidate set as indices into the `Pool`, `filtered(guess)`, scored `suggest`/`suggest_top_k`, private `guess_entropy()`; public `Suggestion<N>` |
| `tree.rs` | `DecisionTree<N>` (crate-private): arena of nodes, one per game position, with a memoized suggestion each |
| `guesser.rs` | `Guess<N>`, `Guesser<N>`: history plus a cursor into a shared `DecisionTree` |
| `game.rs` | `GameResult<N>`, `play()`, private `log_guess()` |

All public types are re-exported from `lib.rs`: `use lex_core::{Guesser, Guess, Strategy, GuessPool, Suggestion, WordCorrectness, Correctness, GameResult, play}`.

## Decision tree (`tree.rs`)

A game position is identified by its guess path: the `(guess word, feedback ordinal)` steps from the
root. For a fixed starting `WordSet` the path determines the candidate set, and `Candidates::suggest`
is deterministic, so a node computes its suggestion once and every later game reaching it reuses
it. Edges are sparse (`HashMap<(Word<N>, u16), NodeId>`: 59,049 possible patterns at N=10) and
keyed by the guess word as well as the pattern, because callers may push guesses that were not the
suggestion (`assist` does). Ordinals are `u16`; a const assertion in `correctness.rs` ties that to
`MAX_WORD_LENGTH`. If `suggest` ever gains randomness or hidden state, the cache is invalid. The `Strategy` is fixed per tree (a `Guesser` takes it at construction), so it is part of the tree's identity.

`Guesser` holds the tree behind `Rc<RefCell<..>>`, so it is not `Send`; `Guesser::reset()` starts a
new game on the same tree, and `play` takes `&mut Guesser` for that reason.

## Strategy and scoring (`strategy.rs`, `candidates.rs`)

The guess pool is immutable (every loaded word); a `Candidates` is a sorted index list into it, so
`filtered` copies indices, not words. `suggest` scores every guess the `Strategy` allows
(`GuessPool::All`, or only the candidates in hard mode) as
`entropy + win_weight * P(guess is the answer)`, with probabilities renormalized over the surviving
candidates each time (`Pool::prior`: softmax of ln-frequency over T, relative to the survivors'
maximum, which keeps hard mode with `win_weight = 0` bit-identical to the pre-strategy solver).
Strict-greater wins; on an exact tie a candidate beats a non-candidate, then frequency order. A
single candidate is returned without scanning the pool. `answer_prior` exposes the same softmax
for reports; the CLI fixes the report prior at `CANONICAL_TEMPERATURE`. Cost is O(pool x candidates)
per node, the reason memoization matters.

`guess_entropy`: for one guess, iterates the candidate words and accumulates probability mass per feedback ordinal (`pattern_ordinal`, no `WordCorrectness` built). Returns entropy in bits.

Uses one thread-local `Patterns { buckets, touched }`, shared by every word length: `buckets` grows to the largest `WordCorrectness::<N>::COUNT` (= 3^N) seen and is all zero between calls; `touched` lists the ordinals a call wrote (a bucket is listed when it is 0.0 before the add, so every prior must be positive). The call sorts `touched` ascending, sums `p * log2(p)` in that order while zeroing each bucket, and clears the list. The ascending order makes the floating-point result independent of word order, which keeps guess sequences bit-identical.

## Correctness algorithm (`pattern_ordinal` in `correctness.rs`)

One generic O(N^2) comparison over `[T; N]` with two `u16` bitmasks and no buffers: pass 1 marks exact matches (`green`); pass 2 walks the non-green guess letters left to right and marks one as misplaced if some unclaimed non-green word letter equals it (`claimed`). It returns the base-3 ordinal directly; `WordCorrectness::correct` decodes it, and a property test checks both against a naive count-based reference. The hot paths (`filtered`, `guess_entropy`) call `pattern_ordinal`.

Measured and rejected (numbers in `benchmarks/micro-opt.md`): alphabet-indexed `[u8; N]` words, and reslicing the bucket buffer to bound-check-free indexing.

## `ordinal()` encoding

`WordCorrectness` is encoded as a base-3 number (position 0 = most significant digit). `Absent = 0, Misplaced = 1, Correct = 2`. All-absent = 0; all-correct = 3^N − 1. This gives a compact index into the pattern probability array.
