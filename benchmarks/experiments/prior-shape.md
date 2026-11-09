# Prior shape

**Verdict: rejected; the power law stays.** No shape improves the headline objective: every
sigmoid loses on every Track, and the floor trades the Corpus prior-weighted mean for the uniform
mean, failing ADR 0007's per-Track guard. No point went to a full Verdict. Issue #50.

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

Run at a262f60 on linden, 2,000 targets per Track with `--seed 0`. Corpus: ngrams snapshot
20200217; `ngrams_sha256` english 5739ccbc, french 02e3de9a, german 66ce1128, spanish 174a2966,
italian 3b9f234c, russian 0df2e046 (prefixes; the full digests are in each report's `corpus`).

## Results

Deltas against `power`, paired by target; negative is better. Prior-weighted mean is the
objective; on `wordle` the Report prior is uniform, so the uniform column is the objective there.

Prior-weighted mean delta, Corpus Tracks:

| Shape | english-5 | french-5 | german-5 | spanish-5 | italian-5 | russian-5 |
|---|---|---|---|---|---|---|
| sigmoid:0.1 | +0.0752 | +0.0542 | +0.0526 | +0.1243 | +0.0797 | +0.1324 |
| sigmoid:0.25 | +0.0595 | +0.0114 | +0.0233 | +0.1068 | +0.1463 | +0.0976 |
| sigmoid:0.5 | +0.0103 | +0.0439 | +0.0083 | +0.0650 | +0.1039 | +0.0435 |
| floor:25 | -0.0002 | +0.0019 | -0.0047 | +0.0026 | -0.0020 | +0.0055 |
| floor:50 | +0.0005 | +0.0040 | -0.0034 | +0.0150 | +0.0082 | +0.0049 |
| floor:75 | +0.0329 | +0.0142 | +0.0119 | +0.0423 | +0.0140 | +0.0309 |

`wordle`, uniform (the objective) and prior-weighted (a readout):

| Shape | wordle (uniform) | wordle (prior) |
|---|---|---|
| sigmoid:0.1 | +0.2060 | +0.1066 |
| sigmoid:0.25 | +0.1000 | +0.0263 |
| sigmoid:0.5 | +0.0910 | +0.0447 |
| floor:25 | -0.0015 | +0.0002 |
| floor:50 | -0.0120 | +0.0086 |
| floor:75 | -0.0115 | +0.0168 |

Uniform mean delta on the Corpus Tracks (a readout, not the objective):

| Shape | english-5 | french-5 | german-5 | spanish-5 | italian-5 | russian-5 |
|---|---|---|---|---|---|---|
| floor:25 | -0.0040 | -0.0030 | -0.0150 | -0.0160 | -0.0155 | +0.0025 |
| floor:50 | -0.0195 | -0.0230 | -0.0329 | -0.0330 | -0.0227 | -0.0270 |
| floor:75 | -0.0395 | -0.0380 | -0.0456 | -0.0330 | -0.0371 | -0.0315 |

## Reading

The screen confirms the expectation: on the Corpus the solver prior already equals the Report
prior, and moving away from it costs prior-weighted guesses.

- **Sigmoid of rank** is worse everywhere, by 0.008 to 0.15 prior-weighted and 0.02 to 0.31
  uniform. Flattening the head of the distribution throws away the frequency signal the Report
  prior rewards; the larger cut-off, closest to the power law, loses least.
- **Frequency floor** moves along the same trade-off as a higher temperature
  ([strategy](strategy.md)): at 50 and 75, flattening the tail lowers the uniform mean on every
  Track and raises the prior-weighted one. `floor:25` averages +0.0005 over the Corpus Tracks,
  with russian-5 at +0.0055, past the 0.005 guard; `floor:50` and `floor:75` average +0.0049 and
  +0.0244, with larger regressions.
- On `wordle`, where uniform is the objective, `floor:50` gains 0.012. Taking it would need a
  Profile-specific prior, which ADRs 0004 and 0008 rule out; the temperature already spans that
  direction.

Recency weighting stays out: the shapes moved only along the uniform versus prior-weighted
trade-off, so the prior's form is not where a gain hides.
