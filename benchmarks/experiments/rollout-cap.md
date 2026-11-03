# Rollout cap

**Verdict: `ROLLOUT_CAP` = 300 candidates, `ROLLOUT_MULTIPLIER` = 8, `rollout_k` = 10 by default.**
At cap 300 the six length-5 headline Tracks fit the 10-minute warm budget together, and every
opening book builds within 2 hours on one core.

Rollout re-scores the top K greedy guesses at a position whose Candidate set holds more than the
Endgame threshold and at most `ROLLOUT_CAP` words. Above the cap the position plays greedy.

## Calibration

PR head `e0ba821` (rollout K=10 by default), main session, Intel i5-7267U (2 cores / 4 threads), one
core, freshly ingested data.

### Multiplier

The book floor reads work at a rollout position as greedy work times `ROLLOUT_MULTIPLIER`. Medians of
`cargo bench -p lex-core --features bench -- rollout::benches`, rollout cold over greedy:

| candidates | rollout / greedy |
|---|---|
| 12 | 2.5 |
| 60 | 10.6 |
| 300 | 18.5 |

The geometric mean is 7.8; counted pattern evaluations on English-5 give 8.16. The multiplier is 8.

### Headline Tracks, warm

| Track | cold s | warm s | warm peak RSS | prior-weighted mean, main to head |
|---|---|---|---|---|
| english-5 | 252 | 164 | 38.5 MB | 4.180 to 4.157 |
| french-5 | 77 | 71 | 26.5 MB | 4.079 to 4.049 |
| german-5 | 30 | 29 | 17.6 MB | 3.934 to 3.908 |
| spanish-5 | 52 | 47 | 23.1 MB | 4.126 to 4.095 |
| italian-5 | 11 | 11 | 11.2 MB | 3.685 to 3.661 |
| russian-5 | 379 | 259 | 58.3 MB | 4.472 to 4.451 |

Together 581 s warm, inside the 10-minute budget; the full regeneration measured 558 s.

### Book builds

The cap was calibrated on Russian-11, the slowest greedy book (1,669 s): at cap 300 it builds in
3,426 s with a 98 MB peak. Under rollout it is not the slowest. The full 113-Track regeneration
(about 10.6 hours) found three books over an hour:

| Track | book build |
|---|---|
| russian-9 | 5,219 s |
| russian-10 | 4,793 s |
| russian-8 | 3,723 s |

Every other book builds in under 30 minutes. A book is a cache built once and reused, so the
full-set gate is 2 hours per book rather than the 60 minutes first agreed, and the cap stays 300.
The likely cause is that the multiplier lifts more positions of these lengths above the book floor:
their book files grow from under 1 KB to 7 to 10 KB.

## Consequence

The slowest warm Track is russian-5 at 249 s; the slowest smoke or breadth Track is french-8 at
101 s. No Track misses the 10-minute warm budget.
