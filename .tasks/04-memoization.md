# 04 — Decision-tree memoization

Depends on: 02 (verification), 03 (corpus identity for any persisted artifact)
Unblocks: 05 (makes unconstrained guessing affordable), full-dictionary
evaluation as the default benchmark mode

## Why

The solver is deterministic, so an n-game simulation is one decision tree
keyed by feedback-pattern sequences. Every game whose feedback history matches
ordinal-for-ordinal has an *identical* candidate set and therefore an
identical next suggestion — but today each game recomputes `suggest`
(O(k²) `correct()` calls) from scratch. `simulate.rs` already exploits this
once (the cached first guess); this task follows the logic to its conclusion.
At N=5 there are only ≤243 distinct first feedback patterns; the tree stays
narrow at every depth.

## Requirements

- Memoize suggestions keyed by the pattern-ordinal sequence (e.g.
  `Vec<u16>` path from the root). The candidate set is fully determined by
  the pattern path given a fixed starting `WordSet` — document this
  invariant and the determinism assumption it rests on (see the comment in
  `simulate.rs` about the first-guess cache; that cache is subsumed here).
- Sparse tree nodes (map keyed by ordinal), not dense 3^N arrays — fan-out is
  243 at N=5 but 59,049 at N=10.
- Invariant to record in code: pattern ordinals fit `u16` because
  3^10 = 59,049 < 65,536; this is tied to `MAX_WORD_LENGTH = 10` and breaks
  at N=11. Add a compile-time or const assertion.
- With memoization, full-dictionary evaluation becomes cheap: make it the
  default benchmark mode (exact expected performance, zero sampling
  variance) and refresh the task-02 baselines to full-dictionary.
- Verification: memoized and unmemoized runs must produce identical
  per-target guess sequences (paired diff via task-02 tooling), plus a
  before/after wall-time measurement.

## Open questions

- Where the memoization lives: inside `Guesser`, or a separate
  simulation-session type that owns a `Guesser` per path (in lex-core or
  lex-cli)? This is an architectural choice — propose with trade-offs.
  Consider that task 05 rewrites `Guesser`'s internals; a design that
  survives that refactor is worth extra thought.
- Eviction/limits: probably unnecessary (the tree is small relative to the
  corpus); confirm with a memory measurement rather than assuming.

## Stretch (confirm with user before starting)

- Persist the tree as a strategy artifact keyed by (lang, N, corpus
  checksum, strategy id + params). Turns the solver into "compile strategy
  once, replay cheaply"; gives `assist` (task 01) instant startup. Suggested
  location: `data/strategies/`.

## Out of scope

- Strategy changes (task 05). Memoize the *current* strategy first and
  re-baseline; 05 then re-baselines again.
- Parallelism (task 10) — but note a shared tree will need synchronization
  there; don't design it into a corner.

## Done when

Full-dictionary English-5 simulation is fast enough to be the default
benchmark, produces per-target results identical to the unmemoized solver,
and the speedup is recorded in the benchmark notes.
