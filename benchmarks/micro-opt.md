# Phase 06: single-threaded micro-optimization

Arbiter: `#[bench]` plus the full English-5 default wall time. An item is adopted if it wins at
least 10% on its own bench without slowing the full run, or if the code gets simpler. After every
item the four full-dictionary oracle runs (default and `--hard-mode --win-weight 0`, English-5 and
French-5, corpus checksum `cf683dcd1747` for English) produced a `games` array identical to the
saved reports: 0 targets changed. The golden tests passed throughout.

Benches are `cargo bench -p lex-core`; wall times are `lex simulate -l 5` over the whole
dictionary, single run on an idle machine unless noted. Baseline is c19419c (the realistic-scale
benches added before any optimization).

## Wall time

| Run | Baseline | Item 2 | Item 1 (final) |
|---|---|---|---|
| English default | 57.07 s | 49.14 s | 39.5 s |
| English hard, λ=0 | 13.67 s | 10.48 s | 11.2 s |
| French default | 22.03 s | 18.36 s | 14.6 s |
| French hard, λ=0 | 5.15 s | 4.00 s | 4.3 s |

English default is 31% faster overall. An alternating A/B of item 1 (two runs each) gave
48.82 / 48.59 s before and 39.55 / 39.44 s after (-19%). Hard mode gives back about 7% of item
2's gain to item 1 (10.48 s to 11.2 s); see item 1.

## Adopted

### Item 2: generic O(N^2) feedback ordinal

`pattern_ordinal<T, N>` compares two `[T; N]` with two `u16` bitmasks and returns the ordinal
directly. It replaces the two-pass `correct()` and its thread-local 64 KB char-count buffer.
`correct()` is now a decode of the ordinal, and a property test checks it against a naive
count-based reference.

| Bench | Before | After |
|---|---|---|
| root_suggest_all_1000_n5 | 15.61 ms | 12.15 ms (-22%) |
| root_suggest_all_300_n10 | 10.71 ms | 10.08 ms (-6%) |
| filtered_1000_n5 | 8.59 us | 9.97 us (+16%) |
| correct, no overlap / partial / perfect | 7.58 / 6.79 / 4.18 ns | ordinal 7.30 / 4.80 / 2.91 ns |

`filtered` got slower (it now computes a full ordinal per word instead of an early-exit
`correct`); it is microseconds per call and off the critical path. `correct()` through the decode
is 12.55 / 10.96 / 10.21 ns, slower than the old one, but only tests and the CLI call it.

### Item 1: selective reset

`guess_entropy` records each ordinal it touches, sorts that list ascending, then sums and zeroes
only those buckets. Summing in ascending ordinal order keeps the floating-point result identical
to the dense scan.

| Bench | Before | After |
|---|---|---|
| root_suggest_all_300_n10 | 10.08 ms | 5.05 ms (-50%) |
| deep suggest, k<=8 of 3000, n5 | 355 us | 198 us (-44%) |
| deep suggest, k<=60 of 3000, n5 | 2.19 ms | 2.32 ms (+6%) |
| root_suggest_all_1000_n5 | 12.15 ms | 13.17 ms (+8%) |
| guess_entropy_10_words | 157.7 ns | 91.7 ns |
| suggest_200_word_dict | 27.5 us | 22.2 us |

The win is at small candidate sets (where most nodes of the decision tree live) and at N=10, where
a dense reset touches 59,049 buckets. A large candidate set at N=5 (243 buckets) pays for the sort
instead. A hybrid that scans densely when the candidate count exceeds the bucket count would
recover the hard-mode regression, but hard mode is not the default and the extra branch was not
worth it.

A latent bug surfaced here: the bucket buffer was one static shared by every word length but sized
from the first length seen, so an N=10 call after an N=5 call indexed past the end. It now grows
to the largest `3^N` seen (committed separately, with a regression test).

### `#[optimize(speed)]` removed

Removed from `guess_entropy` and `lex-data/src/parse.rs` (the one on `correct` went with item 2),
along with both `optimize_attribute` feature gates. Measured as a no-op under `opt-level = 3`:
`guess_entropy` benches moved by under 5% in both directions (root n5 13.17 to 13.08 ms, root n10
5.05 to 5.31 ms, deep k<=60 2.32 to 2.33 ms, deep k<=8 198 to 196 us), and the parse benches did
not move (545 to 543 ns many-years, 128 to 130 ns single). Wall: 39.47 / 11.22 / 14.56 / 4.29 s.

## Rejected

### Item 3: alphabet-indexed `[u8; N]` words

Load-time mapping of each pool letter to a byte, with the `Pool` holding `[u8; N]` words and an
unseen-letter sentinel; `filtered` and `guess_entropy` ran `pattern_ordinal::<u8, N>`. The oracle
was identical x4, but against item 1:

| Bench | Item 1 | Item 3 |
|---|---|---|
| root_suggest_all_1000_n5 | 13.17 ms | 13.51 ms (+3%) |
| root_suggest_all_300_n10 | 5.05 ms | 5.33 ms (+6%) |
| deep suggest, k<=60 | 2.32 ms | 2.38 ms (+2%) |
| deep suggest, k<=8 | 198 us | 195 us (-1%) |
| filtered_1000_n5 | 9.97 us | 8.95 us (-10%) |
| suggest_200_word_dict | 22.2 us | 29.7 us (+34%, alphabet built in `Pool::new`) |

Wall: 39.58 / 11.23 / 14.53 / 4.27 s, against 39.49 / 11.25 / 14.60 / 4.27 s. The comparison is
already over `char`s held in registers, so shrinking them to bytes bought nothing at scalar width.
Phase 09 may still want a byte representation for lanes; it should measure it under SIMD rather
than assume this result carries over.

### Item 4: bounds-check elimination

`assert_unchecked` needs `unsafe`, which the project does not use, so only the safe form was
tried: reslice the bucket buffer to exactly `3^N` once before the loop.

| Bench | Item 1 | Item 4 |
|---|---|---|
| root_suggest_all_1000_n5 | 13.17 ms | 12.52 ms (-5%) |
| root_suggest_all_300_n10 | 5.05 ms | 5.26 ms (+4%) |
| deep suggest, k<=60 | 2.32 ms | 2.26 ms (-3%) |
| deep suggest, k<=8 | 198 us | 190 us (-4%) |

Under the 10% bar with mixed sign. Not adopted, and the `unsafe` form was not attempted.

## Already done

Item 5 (cache log-frequencies) shipped in phase 05 as `Pool.ln_freqs`; no work was needed here.
