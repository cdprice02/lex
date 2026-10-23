# Baselines

Committed `lex simulate` reports that solver changes are measured against. Each is
a sample, kept small enough to commit (a full English-5 report is ~6 MB). Because targets are
paired by word, a full-dictionary run can be compared against these samples too.

| File | Command |
|---|---|
| `english-5.json` | `lex simulate -l 5 --lang english -n 2000 --seed 0 -o benchmarks/english-5.json` |
| `french-5.json` | `lex simulate -l 5 --lang french -n 500 --seed 0 -o benchmarks/french-5.json` |
| `english-5-hard.json` | the same, with `--hard-mode --win-weight 0` (the solver before phase 05) |
| `french-5-hard.json` | the same, with `--hard-mode --win-weight 0` |

The French baseline keeps the generic (language, length) path honest. The unsuffixed files use the
default strategy (unconstrained, T=5, λ=1); the `-hard` files are the original solver, kept as the
reference that `--hard-mode --win-weight 0` must reproduce exactly (0 targets changed). Hard mode
alone does not reproduce them: the win-weight bonus also applies inside the candidate set.

## Full-dictionary runs

Omit `-n` to play every word: exact expected performance with no sampling variance. The decision
tree makes this affordable (English-5, 27,806 games: 14.3 s, 41 MB peak RSS, versus 49 s for the
2000-game sample before the tree). Use it for the headline numbers of a solver change and compare it
to the sampled baselines for the paired diff:

```bash
lex simulate -l 5 --lang english -o /tmp/full.json
lex compare benchmarks/english-5.json /tmp/full.json
```

## Strategy results (phase 05)

English-5, full dictionary (27,806 targets), same corpus (`cf683dcd1747`):

| Strategy | Uniform mean | Prior-weighted mean | Failure rate (>6) | Wall time |
|---|---|---|---|---|
| hard mode, λ=0, T=5 (before) | 4.963 | 4.638 | 12.90% | 14 s |
| unconstrained, λ=1, T=5 (default) | 4.348 | 4.180 | 0.38% | 57 s |

Paired against the same targets: 12,066 better, 4,109 worse, 11,631 unchanged; prior-weighted mean
-0.458 guesses. The 2000-target sample baselines: -0.615 uniform, -0.434 prior-weighted, failure
12.60% to 0.65% (English); -0.448, -0.344, 9.60% to 0.00% (French, 500 targets).

### Temperature and win-weight sweep

`benchmarks/sweep.sh LANG LENGTH OUTDIR` runs the 5 by 5 grid of temperature T and win weight λ on
full-dictionary runs (English-5 and French-5, same corpus as above). The report prior is fixed at
T=5 whatever the solver uses, so cells are comparable, but that favors solver T=5 on the
prior-weighted metric. Rows are T, columns are λ.

English-5, prior-weighted mean guesses (the objective):

| T \ λ | 0 | 0.5 | 1 | 2 | 4 |
|---|---|---|---|---|---|
| 1 | 4.284 | 4.315 | 4.403 | 4.542 | 4.700 |
| 2 | 4.237 | 4.180 | 4.191 | 4.240 | 4.376 |
| 5 | 4.224 | 4.202 | **4.180** | 4.159 | 4.188 |
| 10 | 4.232 | 4.223 | 4.214 | 4.181 | 4.174 |
| 20 | 4.246 | 4.235 | 4.235 | 4.197 | 4.189 |

English-5, uniform mean guesses (guard):

| T \ λ | 0 | 0.5 | 1 | 2 | 4 |
|---|---|---|---|---|---|
| 1 | 4.474 | 4.699 | 4.821 | 4.993 | 5.175 |
| 2 | 4.407 | 4.447 | 4.501 | 4.600 | 4.797 |
| 5 | 4.348 | 4.343 | **4.348** | 4.374 | 4.473 |
| 10 | 4.336 | 4.331 | 4.330 | 4.334 | 4.385 |
| 20 | 4.334 | 4.327 | 4.327 | 4.327 | 4.352 |

English-5, failure rate (guard):

| T \ λ | 0 | 0.5 | 1 | 2 | 4 |
|---|---|---|---|---|---|
| 1 | 0.43% | 5.12% | 8.42% | 12.60% | 16.74% |
| 2 | 0.29% | 0.92% | 2.08% | 4.74% | 9.62% |
| 5 | 0.22% | 0.25% | **0.38%** | 1.14% | 3.61% |
| 10 | 0.20% | 0.20% | 0.23% | 0.65% | 1.92% |
| 20 | 0.21% | 0.21% | 0.21% | 0.56% | 1.20% |

French-5 has the same shape (prior-weighted, uniform, failure at the default: 4.079, 4.233, 0.38%;
best prior-weighted cell T=5, λ=2 at 4.056 with failure 1.10%).

Reading: the win bonus pays off only when the prior is flat enough to trust. At T=1 every λ above 0
hurts, because the solver chases a few frequent words and fails on the tail. From T=5 up, λ trades
a little uniform-mean and failure rate for prior-weighted mean. The default T=5 (the value the
solver already used) is kept, not just inherited: T=2 to 20 are within 0.03 on the prior-weighted
mean at their best λ, and higher T trades a slightly worse prior-weighted mean for a lower failure
rate, so T=5 is a middle choice rather than a sharp optimum. λ=1 is the knee at T=5: it takes 0.044
off the prior-weighted mean at no cost in uniform mean, and λ=2 buys another 0.021 but triples the
failure rate and worsens the uniform mean.

## Comparing a change

Rerun the same command to a scratch path, then:

```bash
lex compare benchmarks/english-5.json /tmp/candidate.json
```

Targets are paired by word, so the comparison is not affected by sampling noise. It warns when
the corpus checksums differ.

## Provenance

KAIKKI Wiktionary extracts are rolling. A report's `corpus.ngrams_sha256` is the corpus checksum from the
cache header (SHA-256 of the records), identifying the exact word list and frequencies it ran against; `corpus.dict` records the Wiktionary download.
A rebuilt corpus will usually have a different checksum, so baselines must be regenerated
(and the change noted) when the corpus is rebuilt, not mixed with reports from an older one.
