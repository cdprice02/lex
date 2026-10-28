# Track targets

**Verdict: targets are set by Tier, not measured per Track.** Headline Tracks play the full
dictionary; breadth Tracks at length 3 play it in full and the other breadth lengths play 500
sampled targets; smoke Tracks play 100 sampled targets, all with seed 0. The rule lives in
`lex-cli/src/tracks.rs`; this note is the evidence for it.

## Calibration

Run at `995e98b` on the reference machine (Intel i5-7267U), cold, with no Opening book. CPU seconds
from child rusage. The macOS screensaver tripped the load gate once before `spanish-6`; the 27
Tracks after it were resumed under `caffeinate -d -i`. The 25 Tracks before the stop match earlier
idle figures (`english-5` 57.1 s against 56.8 s, `wordle-hard` 0.92 s both times), so none was
contended.

**Headline**, all full dictionary:

| Track | CPU s |
|---|---|
| english-5 | 57.1 |
| french-5 | 21.6 |
| german-5 | 7.5 |
| spanish-5 | 13.8 |
| italian-5 | 3.1 |
| russian-5 | 111.1 |
| wordle | 2.0 |
| wordle-all | 12.7 |
| wordle-hard | 0.9 |
| english-5-ultrahard (since dropped) | 17.4 |

**Breadth, length 3:** all six languages ran in full in under 4 s each.

**Breadth, length 8:**

| Track | targets | CPU s |
|---|---|---|
| english-8 | 500 | 403.5 |
| french-8 | full | 406.7 |
| german-8 | full | 187.0 |
| spanish-8 | full | 328.5 |
| italian-8 | full | 140.9 |
| russian-8 | 500 | 601.5, cut off, over budget |

**Smoke** (100 targets): everything finished except `russian-9` and `russian-10`, which hit the
600 s cutoff. The slowest that finished: `russian-7` 521 s, `english-10` 478 s, `english-9` 436 s.

## Why these targets

- **Headline in full.** Every headline Track finished well inside the 10-minute budget of ADR 0007
  (the slowest, `russian-5`, in 111 s), so the exact mean costs nothing to take.
- **Length 3 in full, other breadth lengths sampled.** Length 3 is cheap. At length 8, the full
  dictionary fit for four languages but not for English or Russian, and a rule that picks full or
  sampled per language would be a table of per-Track values tied to this machine. One sampled rule
  for every breadth length above 3 keeps the table derived from the Tier alone.
- **Smoke at 100.** Smoke only has to finish; 100 targets is enough to exercise the length.
- **Seed 0 everywhere**, so a Track's targets are the same on every build and pair by word.

## Over budget

At 145k to 168k words the Russian root alone costs about 2.1e10 to 2.8e10 pattern evaluations
(about 7 to 10 minutes at 48 M/s), so the cold run cannot fit whatever the target count. The Opening
book (ADR 0011) is what these Tracks need; budgets are timed with it built, and recalibrating after
it is in the acceptance of the Opening book's persistence work.

## Dropped

`english-5-ultrahard` is not a Track. On a Corpus run ultrahard plays only the candidates, a path
the unit tests cover; the Wordle Profile's ultrahard Track keeps the Profile path measured.
