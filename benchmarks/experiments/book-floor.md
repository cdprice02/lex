# Opening book floor

**Verdict: `BOOK_FLOOR` = 40,000,000 pattern evaluations**, about one second of single-core scoring,
matching the agreed `assist` latency for a position computed online.

Work at a position is the guesses its Strategy allows times its Candidate set size, with a rollout
multiplier of 1 until rollout lands. A position is booked when its work exceeds the floor.

## Calibration

Main `da6b496`, main session, Intel i5-7267U (2 cores / 4 threads),
`cargo bench --package lex-core -- root_suggest_all deep_suggest_all`, two consecutive runs. A
separate Claude CLI process held ~80% of one logical core throughout; the two runs agree within 2.3%.

| bench | evaluations | run 1 ns/iter | run 2 ns/iter | evals/s |
|---|---|---|---|---|
| root_suggest_all_1000_n5 | 1000 x 1000 = 1,000,000 | 20,924,234 | 20,443,968 | 48.3 M |
| deep_suggest_all_60_of_3000_n5 | 3000 x <=60 | 3,718,485 | 3,598,991 | <=49 M |
| deep_suggest_all_8_of_3000_n5 | 3000 x <=8 | 354,648 | 346,636 | (filter-dominated) |
| root_suggest_all_300_n10 | 300 x 300 = 90,000 | 6,039,224 | 6,073,571 | 14.9 M (length 10) |

Rate: 48.3 M pattern evaluations/s at length 5 (root bench, exact count). `BOOK_FLOOR` =
40,000,000 (rate x 1 s, rounded down to one significant figure).

## Consequence

The `--profile wordle` root costs 12,972 x 2,315 = 3.0e7, below the floor, so that Profile's book is
empty by design (the unit test
`the_production_floor_leaves_the_wordle_profile_unbooked_and_books_english_5` pins the arithmetic).
English-5's root is 27,524 x 27,524 = 7.6e8 and books its top levels. The floor is not retuned for
this. Node count, in-memory size and build time come from `lex-core/tests/book_measure.rs`.
