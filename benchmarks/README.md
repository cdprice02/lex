# Benchmarks

How solver changes are measured, and where the current numbers stand. The objective and the
acceptance rule for a change are in `docs/adr/0007-benchmark-objective.md`.

## Layout

| Path | Holds |
|---|---|
| `README.md` | Methodology and the current results |
| `baselines/*.json` | Committed `lex simulate` reports that changes are paired against |
| `experiments/<topic>.md` | The evidence and verdict behind each solver or performance choice |
| `scripts/` | Harness scripts (`sweep.sh`: temperature by win-weight grid; `oracle.sh`: the Oracle) |

## Current results

Full-dictionary runs, default strategy (unconstrained, T=5, win weight 1, endgame off).
Update this table whenever a change lands.

| Track | Targets | Uniform mean | Prior-weighted mean | Failure rate (>6) | Corpus |
|---|---|---|---|---|---|
| English-5 | 27,524 | 4.3442 | 4.1769 | 0.37% | `29104fb5747b` (phase 08) |
| French-5 | 16,978 | 4.233 | 4.079 | 0.38% | `9b5e427a4e54` (phase 05) |
| Wordle profile | 2,315 | 3.4721 | 3.3930 | 0.00% | `29104fb5747b` |

The Wordle profile is run with `lex simulate --profile wordle` (the 2,315 answers as targets, the
12,972-word Guess pool, the Answer prior at T=5); its headline is the uniform mean. Guess counts:
2 in 59 games, 3 in 1,164, 4 in 1,032, 5 in 60. The `wordle-all` profile, Wordle hard mode, and the
other languages and lengths are not measured yet. The committed
baselines predate the English corpus rebuild (`cf683dcd1747`), so they must be regenerated before
the next paired comparison.

## Hard and ultrahard

Hard mode is the official Wordle rule, as in Alex Selby's `-H` (`okhard` in
[selby_wordle](https://github.com/biorhythmist/selby_wordle)): greens stay in place, and every green
or yellow letter of each earlier guess reappears with at least that multiplicity. His published
optimum for it is 3.5084 mean guesses on the 2,315 answers. Ultrahard (his `-U`) plays only words
consistent with every feedback so far; no published optimum was found for it. Both are reported,
never judged (`docs/adr/0007-benchmark-objective.md`).

| Run | Uniform mean | Failure rate (>6) | Peak RSS | Selby |
|---|---|---|---|---|
| `--profile wordle --hard` | 3.5806 | 0.48% | 8 MB | 3.5084 (`-H`) |
| `--profile wordle --ultrahard` | 3.5823 | 0.48% | 6 MB | none published |

Full 2,315-answer runs on an idle machine, well under the 1 GB cap; timings and the rest of the
figures are in the [measurement comment](https://github.com/cdprice02/lex/pull/68#issuecomment-6452539442).

## Experiments

| Topic | Verdict |
|---|---|
| [Strategy](experiments/strategy.md) | Unconstrained guessing, T=5, win weight 1 |
| [Micro-optimization](experiments/micro-opt.md) | Generic ordinal and selective reset adopted; byte words, bounds-check removal rejected |
| [Endgame](experiments/endgame.md) | Exact search below a threshold, off by default |
| [Partition dedupe](experiments/dedupe.md) | Rejected: slower, and over the memory cap on a full run |
| [Opening book floor](experiments/book-floor.md) | 40 M pattern evaluations (about 1 s single-core); the Wordle Profile books nothing |

## Baselines

Each baseline is a sample, kept small enough to commit (a full English-5 report is ~6 MB). Because
targets are paired by word, a full-dictionary run can be compared against these samples too.

| File | Command |
|---|---|
| `baselines/english-5.json` | `lex simulate -l 5 --lang english -n 2000 --seed 0 -o benchmarks/baselines/english-5.json` |
| `baselines/french-5.json` | `lex simulate -l 5 --lang french -n 500 --seed 0 -o benchmarks/baselines/french-5.json` |
| `baselines/english-5-hard.json` | the same, with `--ultrahard --win-weight 0` (the solver before phase 05) |
| `baselines/french-5-hard.json` | the same, with `--ultrahard --win-weight 0` |

The French baseline keeps the generic (language, length) path honest. The unsuffixed files use the
default strategy; the `-hard` files are the original solver, kept as the reference that
`--ultrahard --win-weight 0` must reproduce exactly (0 targets changed). Ultrahard alone does not
reproduce them: the win-weight bonus also applies inside the candidate set. They were written when
the flag was `--hard-mode`, which is now `--ultrahard`; `--hard` is the official Wordle rule.

## Full-dictionary runs

Omit `-n` to play every word: exact expected performance with no sampling variance. The decision
tree makes this affordable (English-5 in about a minute on one core, 41 MB peak RSS; timings are in the experiments). Use it for the
headline numbers of a change and compare it to the sampled baselines for the paired diff.

## Oracle

A change meant to leave play untouched (a speed change, a refactor) must reproduce the base
commit's games exactly: `simulate -n 200 --seed 0` on every Track of the set, compared on `games`
and `summary`. The sets follow the Tiers of `docs/adr/0007-benchmark-objective.md`.

| Command | Tracks | Run it |
|---|---|---|
| `benchmarks/scripts/oracle.sh <base-ref>` | fast: every language at length 5, `--profile wordle --hard`, English-5 `--ultrahard`, `--profile wordle`, `--profile wordle-all` | on every behavior-preserving PR |
| `benchmarks/scripts/oracle.sh --full <base-ref>` | full: the fast set plus every language at every other Supported length | instead, when the change touches word-length-dependent code (pattern ordinals, counting, the length limit), and once at the end of a map |

Base reports are cached per commit under `data/oracle/<sha>/`, so concurrent runs against one base
share them. A track whose flags the base binary lacks prints `skipped`.

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
