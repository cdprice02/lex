# 08 — Endgame exact search

Depends on: 05 (plugs into the strategy abstraction), 02 (validation)

## Why

Entropy is a heuristic; with few candidates left it is not optimal. When the
candidate set is small, searching the actual game tree (expectimax over
patterns weighted by the answer prior) is trivially affordable and strictly
better. A hybrid — entropy for large sets, exact search below a threshold —
captures most of the remaining gap to known-optimal play (~3.42 average for
unconstrained English-5 with uniform answers; use as a sanity reference, not
a target, since our prior and corpus differ).

## Requirements

- Below a candidate-count threshold, choose the guess minimizing expected
  remaining guesses via exhaustive tree search; above it, keep the phase-05
  scoring.
- The searched guess pool at each node follows the phase-05 policy
  (unconstrained), with the same candidate-preferring tie-break.
- Threshold is a tunable; sweep it against baselines (cost rises fast with
  the threshold — measure where returns diminish).
- Memoization (task 04) applies to search results too — same
  pattern-path keying; avoid recomputing shared subtrees across games.

## Open questions

- Objective: expected guesses (matches the benchmark metric) vs. worst-case
  (minimax, caps the failure tail) vs. expected-with-failure-penalty.
  Recommend expected + report failure rate; confirm with user.
- Initial threshold guess (~15 candidates) — validate empirically.

## Out of scope

- Full-game exact solving from the first guess (cost explodes; the hybrid is
  the point).

## Done when

Paired baseline comparison shows improvement concentrated in the trap-word
tail (e.g. `-atch`, `-ight` families) with no mean regression; threshold
sweep results recorded; new baselines committed.
