# 10 — Multithreading

Depends on: 09 (final phase of the optimization order)

## Why

Phase 3 of the optimization order. Simulation is embarrassingly parallel
across games, and `suggest` is parallelizable across candidates. The
thread-local buffers in `correct()` and `guess_entropy` were designed with
this in mind.

## Notes

Intentionally sparse until scheduled (see 09). Known design constraint to
carry forward: the memoization tree from task 04 becomes shared state here —
whatever structure 04 chooses should not paint this into a corner
(documented in that brief).

## Done when

(To be detailed when scheduled.) Near-linear scaling on games; identical
output to single-threaded runs.
