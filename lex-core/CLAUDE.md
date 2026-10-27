# CLAUDE.md — lex-core

## Purpose

Entropy-based Wordle solver. Pure computation — no I/O. Depends only on `lex-data` for the `Word`/`WordSet` types.

## Module map

| Module | Responsibility |
|---|---|
| `correctness.rs` | `Correctness`, `WordCorrectness<N>`, `correct()`, `ordinal()`, crate-private `pattern_ordinal()` |
| `strategy.rs` | `Strategy` (pool policy, temperature, win weight), `GuessPool`, `answer_prior()`, crate-private `Pool<N>` (immutable guess pool with log-frequencies and the renormalized prior) |
| `candidates.rs` | `Candidates<N>` (crate-private): the shrinking candidate set as indices into the `Pool`, `filtered(guess)`, scored `suggest`/`suggest_top_k`, private `guess_entropy()`; public `Suggestion<N>` |
| `endgame.rs` | `Endgame<N>` (crate-private): exact search for candidate sets at or below `Strategy::endgame_threshold`; see Endgame search below |
| `tree.rs` | `DecisionTree<N>` (crate-private): arena of nodes, one per game position, with a memoized suggestion each |
| `guesser.rs` | `Guess<N>`, `Guesser<N>`: history plus a cursor into a shared `DecisionTree` |
| `game.rs` | `GameResult<N>`, `play()`, private `log_guess()` |

All public types are re-exported from `lib.rs`: `use lex_core::{Guesser, Guess, Strategy, GuessPool, Suggestion, WordCorrectness, Correctness, GameResult, play}`.

## Decision tree (`tree.rs`)

A game position is identified by its guess path: the `(guess word, feedback ordinal)` steps from the
root. For a fixed starting (pool, Answer list) the path determines the candidate set, and `Candidates::suggest`
is deterministic, so a node computes its suggestion once and every later game reaching it reuses
it. Edges are sparse (`HashMap<(Word<N>, u64), NodeId>`: 59,049 possible patterns at N=10) and
keyed by the guess word as well as the pattern, because callers may push guesses that were not the
suggestion (`assist` does). Ordinals are `u64` at every length; a const assertion in `correctness.rs` checks that
3^`MAX_WORD_LENGTH` fits. If `suggest` ever gains randomness or hidden state, the cache is invalid. The `Strategy` is fixed per tree (a `Guesser` takes it at construction), so the tree's identity is (pool, Answer list, `Strategy`).

`Guesser` holds the tree behind `Rc<RefCell<..>>`, so it is not `Send`; `Guesser::reset()` starts a
new game on the same tree, and `play` takes `&mut Guesser` for that reason.

## Endgame search (`endgame.rs`)

Off unless `Strategy::endgame_threshold > 0`. `DecisionTree::suggest` calls `Endgame::best_guess` for
nodes with 2 to threshold candidates and otherwise falls back to `Candidates::suggest`; the tree owns the
`Endgame`, so its memo is shared by every game. Costs are weighted sums using pool-wide prior weights,
so a set's cost is independent of the position it was reached from and the memo is keyed by the sorted
candidate indices alone. Plain objective: sum of weight x guesses to finish. With
`endgame_aversion = L > 0`: sum of weight x a^guesses, a = e^L; the exponential is what keeps the cost
independent of depth (a penalty past guess 6 would not be).

`search` computes a lower bound per guess from its partition (a group needs one guess for the answer it
names and two for each other), sorts by it, and stops once the bound exceeds the best cost. A
non-candidate that leaves the set in one group is skipped, which guarantees the recursion shrinks it.
Ties go to a candidate, then pool order. Children are solved without any bound passed down, so memoized
costs are exact. `win_weight` is ignored here. A top-K entropy probe limit was tried and rejected
(`benchmarks/experiments/endgame.md`). The opt-in test `real_corpus_roots_match_simulated_cost` checks predicted
cost against simulated games on real roots.

## Strategy and scoring (`strategy.rs`, `candidates.rs`)

The guess pool is immutable (every loaded word); a `Candidates` is a sorted index list into it, starting as the whole pool or, when `Guesser::new` gets an Answer list (a Profile's), as exactly those words' pool indices (mapped once through a `HashMap`, sorted ascending so frequency order holds; an answer missing from the pool panics). The prior is always taken over the surviving candidates, so a Profile's Answer list restricts it without changing its formula. `filtered` copies indices, not words. `suggest` scores every guess the `Strategy` allows
(`GuessPool::All`, or only the candidates in hard mode) as
`entropy + win_weight * P(guess is the answer)`, with probabilities renormalized over the surviving
candidates each time (`Pool::prior`: softmax of ln-frequency over T, relative to the survivors'
maximum, which keeps hard mode with `win_weight = 0` bit-identical to the pre-strategy solver).
Strict-greater wins; on an exact tie a candidate beats a non-candidate, then frequency order. A
single candidate is returned without scanning the pool. `answer_prior` exposes the same softmax
for reports; the CLI fixes the report prior at `CANONICAL_TEMPERATURE`. Cost is O(pool x candidates)
per node, the reason memoization matters.

`guess_entropy` dispatches on a `const` memory rule: `entropy_dense` while 3^N x 8 bytes <= `DENSE_BOUND_BYTES` (16 MiB, N <= 13), else `entropy_sorted`. Both are the per-guess counters described below, and a proptest asserts they agree bit for bit. Sort-and-run keeps a thread-local `Vec<(u64, f64)>` scratch (outside the 1 GB cap, ADR 0007), stably sorts it by ordinal so each run's mass accumulates in word order, and sums runs in ascending ordinal through the same `entropy_from_masses` as the dense path. Production reaches it only once `MAX_WORD_LENGTH` exceeds 13.

Dense counter: for one guess, iterates the candidate words and accumulates probability mass per feedback ordinal (`pattern_ordinal`, no `WordCorrectness` built). Returns entropy in bits.

Uses one thread-local `Patterns { buckets, touched }`, shared by every word length: `buckets` grows to the largest `WordCorrectness::<N>::COUNT` (= 3^N) seen and is all zero between calls; `touched` lists the ordinals a call wrote (a bucket is listed when it is 0.0 before the add, so every prior must be positive). The call sorts `touched` ascending, sums `p * log2(p)` in that order while zeroing each bucket, and clears the list. The ascending order makes the floating-point result independent of word order, which keeps guess sequences bit-identical.

## Correctness algorithm (`pattern_ordinal` in `correctness.rs`)

One generic O(N^2) comparison over `[T; N]` with two `u32` bitmasks and no buffers: pass 1 marks exact matches (`green`); pass 2 walks the non-green guess letters left to right and marks one as misplaced if some unclaimed non-green word letter equals it (`claimed`). It returns the base-3 ordinal directly; `WordCorrectness::correct` decodes it, and a property test checks both against a naive count-based reference. The hot paths (`filtered`, `guess_entropy`) call `pattern_ordinal`.

Measured and rejected (numbers in `benchmarks/experiments/micro-opt.md`): alphabet-indexed `[u8; N]` words, and reslicing the bucket buffer to bound-check-free indexing.

## `ordinal()` encoding

`WordCorrectness` is encoded as a base-3 number (position 0 = most significant digit). `Absent = 0, Misplaced = 1, Correct = 2`. All-absent = 0; all-correct = 3^N − 1. This gives a compact index into the pattern probability array.
