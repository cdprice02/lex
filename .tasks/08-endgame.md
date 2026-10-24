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

## Decisions

- **Objective:** minimize true expected guesses under the Answer prior,
  uncapped (no 6-guess cap, no failure penalty), which is what the benchmark
  mean measures. Failure rate (>6) is reported, not optimized. Revised after
  measurement: plain expectation raised the failure rate, so a risk-averse
  variant (minimize the mean of e^(L x guesses), `--endgame-aversion`) was
  added; it keeps the memo keyed by the set alone.
- **Switch:** an Endgame threshold field on `Strategy` (0 = off, the default
  until the sweep justifies a value). `Candidates::suggest` branches on the
  candidate count; the per-node suggestion memo in `DecisionTree` covers
  search results.
- **Search:** guesses come from the full Guess pool (phase 05). A non-candidate
  guess is considered only if it splits the Candidate set into at least two
  feedback patterns; candidates are always considered. A guess stops being
  scored once its partial expected value cannot beat the best so far. An inner
  memo keyed by Candidate set reuses subtrees reached by different Guess paths.
  Guesses are tried in order of a cheap lower bound and the scan stops when the
  bound exceeds the best found; a top-K entropy probe limit was rejected.
- **Determinism:** no epsilon. Candidates are tried first in index order, then
  the rest of the pool in pool order; a guess wins only on strictly lower
  expected guesses. Sums run over feedback ordinals ascending.
  `win_weight` is ignored inside an Endgame.
- **Sweep:** full-dictionary English-5, thresholds {0, 5, 10, 15, 20, 30}, paired
  against the phase-05 default; record in `benchmarks/endgame.md` with a
  per-target breakdown of the trap-word tail. Stop raising the threshold once
  the gain flattens or wall time blows up.
- **Oracle:** at threshold 0 the output must equal the current full-dictionary
  baseline exactly. With the threshold on, the search's predicted value at each
  Endgame root must match the simulated mean over that root's subtree.

## Out of scope

- Full-game exact solving from the first guess (cost explodes; the hybrid is
  the point).

## Done when

Paired baseline comparison shows improvement concentrated in the trap-word
tail (e.g. `-atch`, `-ight` families) with no mean regression; threshold
sweep results recorded; new baselines committed.

## Outcome

Landed as an optional feature, off by default. The criterion of an
improvement concentrated in the trap-word tail with no mean regression was not
met: the best prior-weighted gain is 0.02 guesses, it costs uniform mean and
failure rate, and the `-atch`/`-ight` families do not improve. Results,
the threshold sweep, and the rejected probe limit are in
`benchmarks/endgame.md`.
