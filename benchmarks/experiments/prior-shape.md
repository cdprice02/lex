# Prior shape

**Verdict: pending.** Screens not yet run. Issue #50.

The solver's Answer prior is `p_i` proportional to `f_i^(1/T)` with T = 5 (the power law). On a
Corpus run it already equals the Report prior, so a different shape could only help by
compensating for the greedy entropy search and the rollout. Two alternatives are screened
against it, behind the hidden `--prior-shape` option:

- `sigmoid:C`, the sigmoid of frequency rank from 3Blue1Brown's Wordle solver: words are ranked
  once over the Answer set the prior covers (ties share a rank), weight = logistic((C x set size
  - rank) / width), width fixed at 10% of the set. The temperature has no effect.
- `floor:P`, every frequency below the P-th percentile of the Answer set lifted to that
  percentile's value, then `f^(1/T)`.

Weights are fixed per word for a game; the prior is renormalized over the Candidate set as it
shrinks, never re-ranked. The Report prior stays fixed, so shapes are scored on one yardstick.

## Plan

`benchmarks/scripts/prior-screen.sh DATA_DIR OUTDIR` plays 2,000 targets (`--seed 0`, no
Opening book) per shape on `english-5`, `french-5`, `german-5`, `spanish-5`, `italian-5`,
`russian-5` and `wordle`, and pairs each shape against `power` with `lex compare`. Guess counts
are machine-independent, so the screen runs on linden. Only the best point beyond the screen's
noise goes to a full headline and breadth Verdict (ADR 0007); otherwise the note closes with
the numbers. Recency weighting is out of scope: it needs per-year counts and a new cache format.

Run at: _commit, corpus checksums_.

## Results

Deltas against `power`, paired by target; negative is better. Prior-weighted mean is the
objective; on `wordle` the Report prior is uniform, so the uniform column is the objective there.

Prior-weighted mean delta, Corpus Tracks:

| Shape | english-5 | french-5 | german-5 | spanish-5 | italian-5 | russian-5 |
|---|---|---|---|---|---|---|
| sigmoid:0.1 | | | | | | |
| sigmoid:0.25 | | | | | | |
| sigmoid:0.5 | | | | | | |
| floor:25 | | | | | | |
| floor:50 | | | | | | |
| floor:75 | | | | | | |

Uniform mean delta, `wordle`, and prior-weighted delta, Corpus Tracks, are the guard readouts:

| Shape | wordle (uniform) | wordle (prior) |
|---|---|---|
| sigmoid:0.1 | | |
| sigmoid:0.25 | | |
| sigmoid:0.5 | | |
| floor:25 | | |
| floor:50 | | |
| floor:75 | | |

## Reading

_Pending._
