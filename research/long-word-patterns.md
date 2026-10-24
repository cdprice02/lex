# Pattern representation for long words (issue #12)

Question: how should feedback patterns and words be represented once word length goes past 10?
Throwaway bench: `research/long-word-patterns-bench/` (raw output in
`research/long-word-patterns-bench-output.txt`). Branch is never merged.

## Answer in brief

1. The dense 3^N bucket table is not what limits long words. Counting patterns three ways (dense
   with a touched list, sort-and-run-length, open-addressing hash) costs within roughly 10 to 25%
   of each other at every N from 8 to 14 and every candidate count K from 10 to 50,000. Beyond
   N=14 only sort and hash are possible, and sort is at least as fast as hash at every size
   measured. Dense stops being possible on memory, not speed (3^15 f64 = 115 MB per thread).
2. Computing each pattern is the dominant cost and is where the length-dependent win is. The
   current O(N^2) compare costs 3 to 4 times a letter-count O(N+26) compare at N>=8 (equal at N=5).
   With the cheap pattern, counting is 25 to 40% of a guess score instead of under 10%, so the
   counting method then matters a little more, and sort-and-run is still within 5 to 10% of dense.
3. Ordinal width: `u32` to N=20 (3^20 = 3.49e9 < 2^32), `u64` to N=40. A 2-bit packed key (digit
   Absent=0, Misplaced=1, Correct=2, position 0 in the high bits) fits `u64` to N=32, sorts in the
   same order as the base-3 ordinal (so float sums stay bit-identical), and is built with a shift
   instead of a multiply. Use base-3 up to N=20 where the ordinal also indexes a matrix or a dense
   table; the packed key only matters past 20, which is out of any realistic Supported length.
4. A precomputed guess-by-answer matrix is feasible at 1 GB only for Wordle-sized problems and
   English N<=5. At N>=6 with a full-corpus pool it is 5 GB and up. It is a speed enabler for
   short lengths, not a long-word architecture.
5. Runtime-width words cost 1.3 to 3 times in the pattern hot loop versus const generics in this
   bench (pessimistic: the runtime variant is not inlined). Keep const generics for the short band
   where the loop is hot and sizes are small; a long band can use a different strategy (below).

## Literature and open-source survey

The search turned up little specific to long words; this is the honest state.

- The 3Blue1Brown family of solvers precomputes a guess-by-answer pattern matrix, one byte per
  cell (3^5 = 243 fits `u8`): 12,972 guesses x 2,315 answers is about 30 MB of cells; one write-up
  reports a 160 MB full guess-by-guess variant and 29 MB for the asymmetric one. One Python CLI
  reports a 200 MB compressed or 800 MB uncompressed full table. This matches the arithmetic below.
- Exact and near-optimal Wordle solvers I found all work on the Wordle lists at N=5.
- Lingo solvers (dCode and similar) are constraint filters, not entropy solvers, and cover 5 and 6
  letters. Wordle clones with 6 and 7 letters exist; I found no entropy solver for 11 or more
  letters, and none that describes a representation change. So there is no prior art to copy for
  N>10; the design has to come from the cost model.
- The general technique for counting group sizes without dense buckets is standard: sort keys and
  run-length, hash aggregation, or radix. Which wins is a constants question, answered by the bench.

Sources: https://nhsjs.com/?p=12299, https://pypi.org/project/amitkh-wordle/,
https://github.com/razimantv/wordle-solver, https://flowingdata.com/?p=66664,
https://github.com/boompig/wordle-py.

## Measurements

Single thread, release build, synthetic words with English letter weights (so patterns have
realistic green and yellow overlap), `[u8; N]` words. Each cell is the minimum of 9 trials, in
microseconds per guess (one guess scored against K candidates). Every method's entropy is asserted
bit-identical to the others in the bench.

Caveat on absolute numbers: the machine was shared with other builds (load average 4 to 20 while
running). The repo's own idle benches put `pattern_ordinal` at about 5 ns for N=5; this bench shows
about 28 ns per pair. Use the ratios, not the absolutes. Rerun on an idle machine before quoting a
number in an ADR.

Selected rows (full table in the output file):

| N | K | pattern O(N^2) | pattern O(N+26) | dense | sort | hash | sort + cheap pattern |
|---|---|---|---|---|---|---|---|
| 5 | 1,000 | 28 | 25 | 34 | 49 | 36 | 42 |
| 5 | 50,000 | 1,397 | 1,359 | 1,658 | 2,333 | 1,889 | 1,921 |
| 10 | 1,000 | 258 | 66 | 236 | 276 | 253 | 107 |
| 10 | 50,000 | 12,931 | 3,648 | 11,655 | 13,719 | 15,542 | 5,294 |
| 14 | 10,000 | 2,570 | 757 | 2,844 | 2,930 | 3,107 | 1,083 |
| 16 | 10,000 | 3,297 | 893 | n/a | 3,652 | 4,027 | 1,233 |
| 20 | 1,000 | 441 | 131 | n/a | 468 | 480 | 163 |
| 20 | 50,000 | 21,534 | 6,177 | n/a | 23,622 | 24,064 | 8,251 |
| 24 | 10,000 | 5,955 | 1,786 | n/a | 6,219 | 6,364 | 2,111 |

Readings:

- Counting method: at N=5, dense beats sort by 1.4x at large K (the existing dense path is right
  for short words). From N=8 up, dense, sort and hash are within about 25% of each other and the
  winner flips between rows; there is no crossover length where dense falls off a cliff, because
  the touched list means only populated buckets are visited. The limit is memory.
- Hash is never clearly ahead of sort-and-run; it adds a table sized to 2K. Radix was not
  implemented; at these K it would save at most the sort share, which is under 25% of the cost
  once the pattern cost is included.
- Pattern computation dominates: the pattern pass alone is most of a dense guess score at N>=10 with the
  O(N^2) compare (258 of 236 us at N=10, K=1,000 is within noise of 100%). The letter-count compare is 3.3x faster at N=10 and 3.5x at N=20.
- Const generic versus runtime width (same letter-count pattern, `inline(never)` runtime variant):

| N | K | const | runtime width |
|---|---|---|---|
| 5 | 1,000 | 9.3 | 26.0 |
| 10 | 1,000 | 30.2 | 91.1 |
| 16 | 10,000 | 806 | 1,415 |
| 20 | 10,000 | 1,284 | 1,754 |

  The gap shrinks with N because the loop body grows relative to loop overhead (1.4x at N=20,
  2.5 to 3x at N=5 and 10). This overstates the gap somewhat (the runtime variant was not given
  the chance to specialize), but is the right order of magnitude.
- Compile time and binary size were not measured (the shared machine made compile timings
  meaningless). Qualitatively, each extra monomorphized N instantiates `suggest`, `Endgame`,
  `DecisionTree`, `Guesser` and the CLI paths; 8 lengths today, 18 if 3..=20, so roughly 2.25x the
  generic code. That is a cost to measure with `cargo build --timings` and `cargo bloat` in the
  design ticket, not a number I can report.

## Memory

Dense bucket table (one `f64` per ordinal, one table per thread):

| N | 3^N | f64 table |
|---|---|---|
| 10 | 59,049 | 0.47 MB |
| 12 | 531,441 | 4.3 MB |
| 14 | 4,782,969 | 38 MB |
| 15 | 14,348,907 | 115 MB |
| 16 | 43,046,721 | 344 MB |
| 20 | 3,486,784,401 | 27.9 GB |

Per-thread, so 8 threads at N=15 is 920 MB of scratch, which collides with the 1 GB cap if the cap
is read to include it (ADR 0007 says "precomputed structures", so scratch arguably does not count,
but it is bounded by nothing). Dense is a sound choice only through about N=13 (12.7 MB).

Pattern matrix, guesses x answers, one cell per pair, symmetric pool (pool = answers = P). Cell
width: `u8` to N=5 (3^5 = 243), `u16` to N=10 (59,049), `u32` to N=20, `u64` to N=40.

| Profile | N | P (answers x guesses) | width | matrix | vs 1 GB |
|---|---|---|---|---|---|
| English corpus | 3 | 5k x 5k | u8 | 25 MB | fits |
| English corpus | 4 | 14k x 14k | u8 | 196 MB | fits |
| English corpus | 5 | 28k x 28k | u8 | 784 MB | fits, barely; no room for the pool or tree |
| English corpus | 6 | 52k x 52k | u16 | 5.4 GB | no |
| English corpus | 7 | 85k x 85k | u16 | 14.5 GB | no |
| English corpus | 8 | 120k x 120k | u16 | 28.8 GB | no |
| English corpus | 9 and 10 | 150k x 150k | u16 | 45 GB | no |
| Wordle profile | 5 | 12,972 x 2,315 | u8 | 30.0 MB | fits easily |

Bit-packing (ceil(N log2 3) bits per cell) saves under 20% at N>=7 (45 GB becomes 40 GB at N=9),
so it does not change any verdict. Largest square pool at 1 GB: 31.6k at `u8`, 22.4k at `u16`,
15.8k at `u32`. With 10k answers the guess pool can be 100k (`u8`), 50k (`u16`) or 25k (`u32`).

Build cost is P x A pattern computations. With the letter-count compare at N=10 (about 66 ns under
contention, perhaps 20 ns idle), a 22k square matrix is 484M pairs: 10 to 32 s. A matrix is
therefore a startup cost, not free, and the Decision tree's memoization already amortizes most of
the same work in long games.

A matrix lookup replaces a pattern compute (about 66 ns at N=10 under contention) with a gather
from a row of A cells. For `u8` and A=2,315 the row is 2.3 KB, in L1; for 28k answers it is 28 KB,
in L1 or L2. So it is a 10 to 30x win on the pattern cost wherever it fits, which is why it matters
for N<=5 and Wordle profiles and not at all for N>=8.

## Cost model

Notation: pool size G (guesses scored), candidates K, per-pair pattern cost c_p(N), per-pair
count cost c_c, sort overhead s(K).

    score(node) ~ G * (K * (c_p(N) + c_c) + s(K))
    c_p quadratic compare ~ a * N^2 for N >= 8 (about 20 ns at N=10 to 60 ns at N=24 under contention)
    c_p letter-count      ~ b * (N + 26), about 0.3x the quadratic compare at N >= 8
    c_c dense/sort/hash   ~ 5 to 30 ns contended, within 25% of each other; dense only while 3^N * 8 B is small
    s(K) sort             ~ K log K, under 25% of the total at K >= 1,000 once the pattern is cheap

At every length the dominant term is G * K * c_p. Counting representation moves the total by under
25%; pattern computation moves it 3x; shrinking G or K (candidate pruning, restricted guess pools,
the existing Decision tree and Endgame) moves it by orders of magnitude. The breadth tier in ADR
0007 caps assist at 1 s per move, so at N=20 with G = K = 10,000 (1e8 pairs at about 15 ns each
with the cheap pattern, 1.5 s per unrestricted root move) the lever is a restricted guess pool, not
the counting method.

## Recommendation per length band

| Band | Pattern compute | Counting | Ordinal / key | Words | Matrix |
|---|---|---|---|---|---|
| 3 to 5 | keep O(N^2) (equal to letter-count at N=5) | dense + touched list (current) | `u16` ordinal today (`u8` suffices to N=5) | const generic `Word<N>` | optional speed enabler: `u8` matrix for Wordle profile (30 MB) and English N<=5 (up to 784 MB); a decision for #19 |
| 6 to 10 | letter-count O(N+26), about 3x faster at N>=8 | dense + touched list (3^10 f64 = 0.47 MB per thread) | `u16`, as now | const generic | not feasible at corpus scale (5 GB at N=6); only for a restricted pool |
| 11 to 20 | letter-count | sort-and-run-length on `(u32, f64)`; no per-thread table; dense is possible to N=13 but buys nothing | `u32` ordinal (fits to N=20); widen the const assertion and the Decision tree edge key | const generic if compile budget allows, else runtime-width `Box<[char]>` or flat `[u8]` for this band only (it costs 1.4x at N=20 in the pattern loop, a minor penalty because the band is dominated by G * K anyway) | no |
| 21 to 40 | letter-count | sort-and-run-length on `(u64, f64)` | `u64` base-3 to N=40, or 2-bit packed `u64` to N=32 | runtime width | no |

Pushing the dispatch decision: the pattern change (letter-count) is the cheapest, safest change and
benefits N>=8 immediately; it needs a `Word` alphabet-index representation (`[u8; N]`), which the
phase-06 micro-opt rejected at N<=10 for English only because it did not pay there. For long words
it does pay, so the rejection was length specific, not general.

Equivalence oracle for any of this: all three counting methods and both pattern functions return
bit-identical entropy in the bench (asserted), provided each sums in ascending ordinal order. The
2-bit packed key preserves base-3 order, so even a key change would leave guess sequences
unchanged.

## What this changes for neighboring decisions

- #16 (long-word architecture): the pattern ordinal and counting are not the obstacle; the
  `u16` ordinal limit and dense table are cheap to lift (`u32` ordinal, sort-and-run past N=13).
  Past N=10 the real question is G * K per node: the architecture should centre on restricting the
  guess pool or sampling it, not on representation. A runtime-width band for N>10 is affordable.
- #19 (speed enablers): the letter-count pattern (3x at N>=8) is a larger and simpler speed-up than
  any counting change; a matrix helps only N<=5 / Wordle (30 MB to 784 MB, `u8`).
- ADR 0007: dense scratch is per thread, so the 1 GB cap should say whether it counts scratch.
