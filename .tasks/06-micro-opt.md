# 06 — Single-threaded micro-optimization

Depends on: 03 (header room for the alphabet table), 05 (hot loop is stable
after the strategy refactor), benches as the arbiter throughout
Unblocks: 09 (SIMD needs the alphabet-indexed representation)

## Why

Phase 1 of the optimization order (single-threaded → SIMD → multithreading).
Every item here must be justified by a before/after `#[bench]` measurement —
several are plausible-but-unproven, and one existing annotation is already
known to be decorative (`#[optimize(speed)]` does nothing under
`opt-level = 3`; see ROADMAP nightly notes).

## Items (each independently measurable; order within the task is open)

1. **Selective reset in `guess_entropy`** (lex-core/src/guesser.rs): the
   thread-local `PATTERN_PROBS` is `fill(0.0)`-ed across all 3^N buckets per
   candidate — 472 KB of memset at N=10, per candidate, while typically only
   a handful of ordinals are touched. Track touched ordinals and zero only
   those — the same trick `correct()` already uses for its char counts.
2. **`correct()` alternative** (lex-core/src/correctness.rs): the current
   two-pass uses a thread-local 64 KB BMP count buffer with scattered cache
   lines. For N ≤ 10 a plain O(N²) nested loop over two stack arrays
   (≤100 comparisons, ~80 bytes, no `thread_local`/`RefCell`) may win.
   Benchmark both; keep the winner, delete the loser.
3. **Alphabet-indexed word representation**: map each language's alphabet to
   small `u8` indices, making words `[u8; N]` — 4× smaller working set,
   byte-wise `correct()`, lane-ready for SIMD (task 09). The alphabet table
   is derived from the corpus at build time.
4. **Bounds-check elimination**: `hint::assert_unchecked` (stable) on
   `probs[ordinal]` — ordinal < 3^N is provable from `ordinal()`'s
   construction.
5. **Cache log-frequencies** at `WordSet` load so candidate-set updates don't
   recompute `ln`/`exp` per word per `push_guess` (exact shape depends on
   where task 05 moved the prior).

## Open questions

- Item 3, the big one: does the *on-disk* format store u8 indices (alphabet
  table in the task-03 header, format version bump) or do we map at load
  time only (disk stays u32 scalars, `WordSet` holds the mapped
  representation)? Load-time mapping is simpler and captures the hot-loop
  wins; on-disk indexing shrinks files 4× but couples format to alphabet
  stability. Propose with trade-offs.
- Whether `#[optimize(speed)]` annotations are kept as insurance or removed.

## Constraints

- Solver output must be bit-identical: verify with the golden test and a
  paired baseline diff (these are optimizations, not behavior changes).
- Record before/after bench numbers (in the PR/commit description or a notes
  file under `benchmarks/`).

## Out of scope

- SIMD (task 09) and threading (task 10).
- `generic_const_exprs` stack buffers — listed in ROADMAP as
  experiment-grade; only attempt with user sign-off.

## Done when

Each adopted item has committed bench numbers showing a win; rejected items
are noted with their numbers so they aren't re-proposed later.
