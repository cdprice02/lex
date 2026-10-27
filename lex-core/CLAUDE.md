# CLAUDE.md — lex-core

## Purpose

Entropy-based Wordle solver. Pure computation — no I/O. Depends only on `lex-data` for the `Word`/`WordSet` types.

## Module map

| Module | Responsibility |
|---|---|
| `correctness.rs` | `Correctness`, `WordCorrectness<N>`, `correct()`, `ordinal()`, crate-private `pattern_ordinal()`, `Code<N>` |
| `strategy.rs` | `Strategy` (pool policy, temperature, win weight), `GuessPool` (`All`, `Hard`, `Ultrahard`), `answer_prior()`, crate-private `Pool<N>` (immutable guess pool with log-frequencies, Alphabet codes and the renormalized prior) |
| `allowed.rs` | `Allowed` (crate-private): the guesses a node permits (`Whole`, `Candidates`, or a sorted `List`), derived per feedback; `HardRule`, Selby's `okhard` |
| `candidates.rs` | `Candidates<N>` (crate-private): the shrinking candidate set as indices into the `Pool`, `filtered(guess)`, scored `suggest`/`suggest_top_k`, private `guess_entropy()`; public `Suggestion<N>` |
| `endgame.rs` | `Endgame<N>` (crate-private): exact search for candidate sets at or below `Strategy::endgame_threshold`; see Endgame search below |
| `book.rs` | `OpeningBook<N>` (preorder nodes of guess pool index and `(ordinal, child)` edges), `BOOK_FLOOR` |
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

## Opening book (`book.rs`, `tree.rs`)

`Guesser::build_book()` books, top-down, every position whose work (guesses its `Allowed` permits x candidate count) exceeds `BOOK_FLOOR` (40 M pattern evaluations, `benchmarks/experiments/book-floor.md`); `build_book_with_floor` takes the floor so tests can lower it. Work never rises along a path, so the booked positions are a subtree. Each booked node stores the guess `DecisionTree::suggest` chose; a child is booked only if its own work exceeds the floor. `Guesser::with_book` seeds a tree: a node created by playing the booked guess with a booked edge takes the booked child's guess as its pre-filled suggestion and stays on the book; any other guess leaves it. The book holds no Candidate sets, so nodes still filter on creation. The test-only `booked_computations` counter proves a seeded run never searches at a booked node. A book is valid only for the pool, Answer list and Strategy it was built for (the key is #46's).

## Endgame search (`endgame.rs`)

Off unless `Strategy::endgame_threshold > 0`. `DecisionTree::suggest` calls `Endgame::best_guess` for
nodes with 2 to threshold candidates and otherwise falls back to `Candidates::suggest`; the tree owns the
`Endgame`, so its memo is shared by every game. Costs are weighted sums using pool-wide prior weights,
so a set's cost is independent of the position it was reached from. The memo is keyed by the sorted
candidate indices and the allowed guesses (a cost under fewer allowed guesses must not be reused for more); an
`Allowed::List` is interned to a small id, and in normal mode and on an ultrahard Corpus run the allowed set is implied, so the key is the candidates alone. Plain objective: sum of weight x guesses to finish. With
`endgame_aversion = L > 0`: sum of weight x a^guesses, a = e^L; the exponential is what keeps the cost
independent of depth (a penalty past guess 6 would not be).

`search` computes a lower bound per guess from its partition (a group needs one guess for the answer it
names and two for each other), sorts by it, and stops once the bound exceeds the best cost. A
non-candidate that leaves the set in one group is skipped, which guarantees the recursion shrinks it.
Ties go to a candidate, then pool order. Children are solved without any bound passed down, so memoized
costs are exact. `win_weight` is ignored here. A top-K entropy probe limit was tried and rejected
(`benchmarks/experiments/endgame.md`). The opt-in test `real_corpus_roots_match_simulated_cost` checks predicted
cost against simulated games on real roots.

## Allowed guesses (`allowed.rs`)

Each tree node stores an `Allowed` beside its candidates, a function of the guess path built by `Allowed::after(mode, pool, guess, feedback ordinal)` from its parent. `GuessPool::All` keeps `Whole` (every pool word). `Hard` filters the parent's allowed words by `HardRule`, Selby's `okhard`: the guess's greens stay in place, and its green and yellow letters reappear with at least the scored multiplicity (grays may be replayed, a yellow may repeat its position). `Ultrahard` keeps the words that score the guess exactly as played; on a Corpus run (the root candidates are the whole pool) the allowed set is the candidates at every depth, so the node stores `Candidates` and nothing extra. Otherwise a node holds a sorted index list that always contains its candidates. `Guesser::rule_violation(word)` re-derives the same rule from the history for `assist`, with a one-line reason.

## Strategy and scoring (`strategy.rs`, `candidates.rs`)

The guess pool is immutable (every loaded word); a `Candidates` is a sorted index list into it, starting as exactly the Answer list's pool indices (`Guesser::new` always takes one: every pool word on a Corpus run, a Profile's list otherwise; mapped once through a `HashMap`, sorted ascending so frequency order holds; an answer missing from the pool panics). The prior is always taken over the surviving candidates, so a Profile's Answer list restricts it without changing its formula. `filtered` copies indices, not words. `suggest` scores every guess the `Strategy` allows
(see Allowed guesses below) as
`entropy + win_weight * P(guess is the answer)`, with probabilities renormalized over the surviving
candidates each time (`Pool::prior`: softmax of ln-frequency over T, relative to the survivors'
maximum, which keeps ultrahard with `win_weight = 0` bit-identical to the pre-strategy solver).
Strict-greater wins; on an exact tie a candidate beats a non-candidate, then frequency order. A
single candidate is returned without scanning the pool. `answer_prior` exposes the same softmax
for reports; the CLI fixes the report prior at `CANONICAL_TEMPERATURE`. Cost is O(pool x candidates)
per node, the reason memoization matters.

`guess_entropy` dispatches on a `const` memory rule: `entropy_dense` while 3^N x 8 bytes <= `DENSE_BOUND_BYTES` (16 MiB, N <= 13), else `entropy_sorted`. Both are the per-guess counters described below, and a proptest asserts they agree bit for bit. Sort-and-run keeps a thread-local `Vec<(u64, f64)>` scratch (outside the 1 GB cap, ADR 0007), stably sorts it by ordinal so each run's mass accumulates in word order, and sums runs in ascending ordinal through the same `entropy_from_masses` as the dense path. Production reaches it only once `MAX_WORD_LENGTH` exceeds 13.

Dense counter: for one guess, iterates the candidate words and accumulates probability mass per feedback ordinal (`pattern_ordinal`, no `WordCorrectness` built). Returns entropy in bits.

Uses one thread-local `Patterns { buckets, touched }`, shared by every word length: `buckets` grows to the largest `WordCorrectness::<N>::COUNT` (= 3^N) seen and is all zero between calls; `touched` lists the ordinals a call wrote (a bucket is listed when it is 0.0 before the add, so every prior must be positive). The call sorts `touched` ascending, sums `p * log2(p)` in that order while zeroing each bucket, and clears the list. The ascending order makes the floating-point result independent of word order, which keeps guess sequences bit-identical.

## Correctness algorithm (`pattern_ordinal` in `correctness.rs`)

Letter count over the Corpus Alphabet at every length (ADR 0010). `Pool::new` maps each word through `WordSet::alphabet()` (sorted letters, index = position) to a `Code<N> = [u8; N]` stored beside the `Word`; a letter outside the Alphabet encodes as `UNKNOWN_LETTER` (255), which no pool word holds, so an `assist` guess with foreign letters scores as absent for them. An Alphabet over 255 letters is a typed `LexDataError::AlphabetTooLarge` at `DataDir::load`; `Pool::new` panics on one built some other way.

`pattern_ordinal(word, guess, counts)`: pass 1 tallies each non-green word letter in `counts` (a `[u8; 256]`); pass 2 walks the guess left to right: green if equal, else misplaced when the letter's tally is positive (spending one), else absent. It then zeroes the entries for the word's letters, so `counts` is all zero between calls and a caller makes one on its stack per scan (`filtered`, each entropy scan, `Endgame::partition`). Linear in N. `WordCorrectness::correct` takes `Word`s, so it indexes letters by first appearance in the pair (at most 2N values) and calls the same function. `compare_ordinal` (test-only) is the old O(N^2) compare, kept as an oracle and as the "old" side of the `ordinal_count_n*` / `ordinal_compare_n*` benches (N = 5, 10, 13, 25).

The earlier rejection of alphabet-indexed `[u8; N]` words (`benchmarks/experiments/micro-opt.md`) was a length-5 speed result; the single-technique decision in ADR 0010 supersedes it, gated on English-5 CPU time in the PR that introduced this.

## `ordinal()` encoding

`WordCorrectness` is encoded as a base-3 number (position 0 = most significant digit). `Absent = 0, Misplaced = 1, Correct = 2`. All-absent = 0; all-correct = 3^N − 1. This gives a compact index into the pattern probability array.
