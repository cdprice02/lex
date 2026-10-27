# Partition dedupe with inherited guess groups

**Verdict: rejected.** Slower on the full English-5 run and over the 1 GB memory cap; reverted.

Change tried: pool words that split the candidate set identically share a group; children inherit
the groups, and `suggest`, `suggest_top_k` and the Endgame score one representative per group.
Output was byte-equal to the undeduped solver by construction and by test. The code is in the
history of the revert.

## Full English-5 A/B

`lex simulate -l 5 --lang english --seed 0` (all 27,524 targets), `/usr/bin/time -l`, idle
machine, 3 rounds alternating the builds; games identical to the base.

| Build | user s | real s | peak RSS MB |
|---|---|---|---|
| base `c44f4e8` | 71.11, 69.04, 69.01 | 73.2, 70.1, 70.1 | 34, 37, 37 |
| dedupe `5397287` | 86.67, 82.60, 82.68 | 101.8, 94.9, 95.4 | 1508, 1644, 2059 |

User time rose about 20%. Memory grew because every Decision tree node kept its own copy of the
groups, a list the size of the Guess pool once anything merged, cloned into each child. Peak RSS
also varied between identical deterministic runs, which was never explained. Sampled runs
(`-n 500`) stayed small (6.1 to 207 MB English-5, 14.5 to 361 MB Russian-6), which hid the
full-dictionary cost.

## Profile (samply, self time by function)

`samply record` on the default strategy, `--seed 0`, 1 kHz, release binaries, base = `1da882e`,
taken under contention (shares of samples, not wall time). English-5 `-n 200`, Russian-6 `-n 60`.

| Function | base En-5 | dedupe En-5 | base Ru-6 | dedupe Ru-6 |
|---|---|---|---|---|
| `pattern_ordinal` | 76.0% | 78.9% | 82.0% | 82.3% |
| entropy counter (`guess_entropy` / `entropy_dense`) | 22.2% | 13.7% | 17.1% | 14.2% |
| `Guesser::suggest` (inlined scoring loop, hashing) | 0.1% | 4.8% | 0.0% | 2.4% |
| group bookkeeping (`coarsened`, `group`, sorts) | 0 | 0.7% | 0 | 0.1% |
| total samples | 46,223 | 44,546 | 253,042 | 254,774 |

Feedback computation (`pattern_ordinal`) is about 80% of default play either way. Most scoring
is at shallow positions whose candidate sets are still large, where few pool words are
interchangeable, so skipping merged representatives saves little, while merge detection (a
second feedback vector per non-candidate representative) costs about as much as it saves.

## Revisit when

Both hold:

- an Opening book serves the shallow positions, leaving live scoring to deep positions where
  groups do merge;
- merges can be found more cheaply than a second feedback vector per guess, with groups shared
  between nodes rather than cloned.
