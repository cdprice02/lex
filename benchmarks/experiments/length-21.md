# Word lengths 11 to 21

**Verdict: every Supported length from 11 to 21 runs warm in under 31 seconds**, far inside the
10-minute Track budget. The cost is the cold run, which is almost all opening book build: 2.9 hours
for all 60 Tracks, built once per book key.

## Setup

PR branch at `92baa60`, main session, Intel i5-7267U (2 cores / 4 threads), a fresh data directory
ingested by this branch (all six languages to length 21). Each Track ran alone through the `tracks`
test binary: once cold (no book on disk, `INSTA_UPDATE=always`, so the run builds the book and
writes the snapshot), then once warm (book on disk, `INSTA_UPDATE=no`, so the run must reproduce
the snapshot). Every warm run passed. English-11 and English-12 first ran beside a ~10% CPU
watcher process and were rerun cold on the idle machine (526 s to 524 s, 405 s to 391 s; both
rebuilt books byte-identical); the idle figures are below. Times are whole seconds.

## Results

Cold / warm seconds per Track; (b) marks a breadth Track, the others are smoke. A blank cell is a
length past the language's largest Supported Corpus.

| N | English | French | German | Spanish | Italian | Russian |
|---|---|---|---|---|---|---|
| 11 | 524 / 7 | 237 / 4 | 475 / 9 | 177 / 3 | 140 / 4 | 1669 / 28 |
| 12 | 391 / 4 | 160 / 3 | 543 / 6 | 105 / 2 | 93 / 1 | 1329 / 10 |
| 13 | 279 / 9 (b) | 94 / 4 (b) | 503 / 18 (b) | 51 / 2 (b) | 49 / 3 (b) | 923 / 31 (b) |
| 14 | 165 / 0 | 40 / 1 | 430 / 2 | 17 / 0 | 18 / 1 | 632 / 3 |
| 15 | 85 / 0 | 15 / 0 | 288 / 1 | 2 / 2 | 3 / 4 | 331 / 2 |
| 16 | 36 / 0 | 3 / 2 | 165 / 1 | 1 / 1 | 1 / 1 | 157 / 1 |
| 17 | 15 / 0 | 1 / 1 | 95 / 1 | 0 / 0 | 0 / 0 | 67 / 0 |
| 18 | 3 / 3 | 0 / 1 | 49 / 0 | 0 / 0 | 0 / 0 | 25 / 1 |
| 19 | 2 / 1 | 0 / 0 | 23 / 0 |  | 0 / 0 | 5 / 6 |
| 20 | 0 / 1 | 0 / 0 | 6 / 6 |  |  | 1 / 2 |
| 21 | 0 / 0 (b) |  | 2 / 3 (b) |  |  | 1 / 0 (b) |

Cold total 10,426 s (2.9 h); warm total 196 s. At the longest lengths the root falls under
`BOOK_FLOOR`, the book is empty, and cold and warm are the same run (equal within the 1 s
resolution). Cold time tracks root work: Russian-11 (150,262 words) is the largest at 28 minutes.

German-11, German-13 and German-21, the Tracks named in the length-21 budget check, run warm in
9 s, 18 s and 3 s.

## Build cost

Main `3d6314d` against this branch at `76f4c0d`, same machine, one tree after the other.
`hyperfine --runs 3 --prepare 'cargo clean' 'cargo build --release'`, then one
`cargo build --release --timings` and `cargo bloat --release --crates`.

| | main | branch | ratio |
|---|---|---|---|
| clean release build, mean of 3 | 176.9 s (sd 8.2) | 215.8 s (sd 1.3) | 1.22 |
| `lex-cli` lib, `--timings` | 9.6 s | 19.7 s | 2.05 |
| `lex` bin, `--timings` | 77.2 s | 101.0 s | 1.31 |
| `lex` binary | 7,377,632 B | 9,101,456 B | 1.23 |
| `.text` | 4.2 MiB | 5.4 MiB | 1.29 |
| `lex_cli` in `.text` (`cargo bloat`) | 936.6 KiB | 2.1 MiB | 2.3 |

`match_word_length!` now monomorphizes 19 lengths instead of 8, and the growth lands in
`lex-cli`, the crate that instantiates the generic solver per length. The whole build and the
whole binary each grow by under a quarter, inside the agreed bound (neither may double), so
the dispatch stays one monomorphization per length.
