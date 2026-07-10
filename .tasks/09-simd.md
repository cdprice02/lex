# 09 — SIMD

Depends on: 06 (alphabet-indexed `[u8; N]` representation), and completion of
phases 00–08 per the optimization order (single-threaded first)

## Why

Phase 2 of the optimization order. `portable_simd` (nightly `std::simd`) over
the byte-indexed word representation — e.g. batching `correct()` across
multiple target words per candidate, or vectorizing the entropy
accumulation. This is the feature that makes the nightly-toolchain decision
load-bearing (see ROADMAP decisions).

## Notes

This brief is intentionally sparse: detailed planning now would go stale
across phases 02–08 (the hot loop, word representation, and strategy all
change before this starts). Detail it against the then-current code, with
bench-driven scope like task 06.

## Done when

(To be detailed when scheduled.) Bench-verified speedup on `suggest` and
full-simulation wall time; bit-identical solver output.
