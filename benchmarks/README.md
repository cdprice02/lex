# Benchmarks

How solver changes are measured, and where the current numbers stand. The objective and the
acceptance rule for a change are in `docs/adr/0007-benchmark-objective.md`.

## Layout

| Path | Holds |
|---|---|
| `README.md` | Methodology and the current results |
| `baselines/*.json` | Committed `lex simulate` reports that changes are paired against |
| `experiments/<topic>.md` | The evidence and verdict behind each solver or performance choice |
| `scripts/` | Harness scripts (`sweep.sh`: temperature by win-weight grid; `oracle.sh <base-ref>`: the Oracle, byte-equal `simulate` output against a base commit) |

## Current results

Full-dictionary runs, default strategy (unconstrained, T=5, win weight 1, endgame off).
Update this table whenever a change lands.

| Track | Targets | Uniform mean | Prior-weighted mean | Failure rate (>6) | Corpus |
|---|---|---|---|---|---|
| English-5 | 27,524 | 4.3442 | 4.1769 | 0.37% | `29104fb5747b` (phase 08) |
| French-5 | 16,978 | 4.233 | 4.079 | 0.38% | `9b5e427a4e54` (phase 05) |

The Wordle profiles and the other languages and lengths are not measured yet. The committed
baselines predate the English corpus rebuild (`cf683dcd1747`), so they must be regenerated before
the next paired comparison.

## Experiments

| Topic | Verdict |
|---|---|
| [Strategy](experiments/strategy.md) | Unconstrained guessing, T=5, win weight 1 |
| [Micro-optimization](experiments/micro-opt.md) | Generic ordinal and selective reset adopted; byte words, bounds-check removal rejected |
| [Endgame](experiments/endgame.md) | Exact search below a threshold, off by default |

## Baselines

Each baseline is a sample, kept small enough to commit (a full English-5 report is ~6 MB). Because
targets are paired by word, a full-dictionary run can be compared against these samples too.

| File | Command |
|---|---|
| `baselines/english-5.json` | `lex simulate -l 5 --lang english -n 2000 --seed 0 -o benchmarks/baselines/english-5.json` |
| `baselines/french-5.json` | `lex simulate -l 5 --lang french -n 500 --seed 0 -o benchmarks/baselines/french-5.json` |
| `baselines/english-5-hard.json` | the same, with `--hard-mode --win-weight 0` (the solver before phase 05) |
| `baselines/french-5-hard.json` | the same, with `--hard-mode --win-weight 0` |

The French baseline keeps the generic (language, length) path honest. The unsuffixed files use the
default strategy; the `-hard` files are the original solver, kept as the reference that
`--hard-mode --win-weight 0` must reproduce exactly (0 targets changed). Hard mode alone does not
reproduce them: the win-weight bonus also applies inside the candidate set.

## Full-dictionary runs

Omit `-n` to play every word: exact expected performance with no sampling variance. The decision
tree makes this affordable (English-5 in about a minute on one core, 41 MB peak RSS; timings are in the experiments). Use it for the
headline numbers of a change and compare it to the sampled baselines for the paired diff.

## Comparing a change

Rerun the same command to a scratch path, then:

```bash
lex compare benchmarks/baselines/english-5.json /tmp/candidate.json
```

Targets are paired by word, so the comparison is not affected by sampling noise. It warns when
the corpus checksums differ.

## Provenance

KAIKKI Wiktionary extracts are rolling. A report's `corpus.ngrams_sha256` is the Corpus checksum
from the cache header (SHA-256 of the records), identifying the exact word list and frequencies it
ran against; `corpus.dict` records the Wiktionary download. A rebuilt Corpus will usually have a
different checksum, so baselines must be regenerated (and the change noted) when the Corpus is
rebuilt, not mixed with reports from an older one.
