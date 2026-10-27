# Partition dedupe with inherited guess groups

Change: pool words that split the candidate set identically share a group; children inherit the
groups and `suggest`, `suggest_top_k` and the Endgame score one representative per group (see
`lex-core/CLAUDE.md`, Partition dedupe). Output is byte-equal to the undeduped solver by
construction and by test.

## Profile (samply, self time by function)

`samply record` on the default strategy, `--seed 0`, 1 kHz, release binaries, base = `1da882e`,
with the Oracle and other jobs running on the machine (shares of samples, not wall time).
English-5 `-n 200`, Russian-6 `-n 60`.

| Function | base En-5 | dedupe En-5 | base Ru-6 | dedupe Ru-6 |
|---|---|---|---|---|
| `pattern_ordinal` | 76.0% | 78.9% | 82.0% | 82.3% |
| entropy counter (`guess_entropy` / `entropy_dense`) | 22.2% | 13.7% | 17.1% | 14.2% |
| `Guesser::suggest` (inlined scoring loop, hashing) | 0.1% | 4.8% | 0.0% | 2.4% |
| group bookkeeping (`coarsened`, `group`, sorts) | 0 | 0.7% | 0 | 0.1% |
| total samples | 46,223 | 44,546 | 253,042 | 254,774 |

Reading: feedback computation (`pattern_ordinal`) is about 80% of default play in both targets
and the total sample count barely moves (-3.6% English-5, +0.7% Russian-6). Most scoring work is
at shallow positions whose candidate sets are still large, where few pool words are
interchangeable, so skipping merged representatives saves little while the merge detection pass
(a second feedback vector per non-candidate representative) costs about as much as it saves.

## Peak RSS (`/usr/bin/time -l`, `-n 500 --seed 0`, default strategy)

| Target | base | dedupe |
|---|---|---|
| English-5 | 6.1 MB | 207 MB |
| Russian-6 | 14.5 MB | 361 MB |

Per-node groups are well under the 1 GB cap, so the "drop a node's groups once its children are
expanded" step was not needed.

## Speed bar

Pending: the gate and Speed bar timings (full English-5, Russian-6 `-n 500 --seed 0`, 3 or more
alternating A/B runs) run in the main session on an idle machine. A contended single run during
the RSS measurement above gave English-5 48.5 s (base) vs 53.6 s (dedupe) and Russian-6 366.5 s vs
417.4 s; those are not valid timings, but they do not suggest a win.
