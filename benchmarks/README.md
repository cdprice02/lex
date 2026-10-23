# Baselines

Committed `lex simulate` reports that solver changes are measured against. Each is
a sample, kept small enough to commit (a full English-5 report is ~6 MB). Because targets are
paired by word, a full-dictionary run can be compared against these samples too.

| File | Command |
|---|---|
| `english-5.json` | `lex simulate -l 5 --lang english -n 2000 --seed 0 -o benchmarks/english-5.json` |
| `french-5.json` | `lex simulate -l 5 --lang french -n 500 --seed 0 -o benchmarks/french-5.json` |

The French baseline keeps the generic (language, length) path honest.

## Full-dictionary runs

Omit `-n` to play every word: exact expected performance with no sampling variance. The decision
tree makes this affordable (English-5, 27,806 games: 14.3 s, 41 MB peak RSS, versus 49 s for the
2000-game sample before the tree). Use it for the headline numbers of a solver change and compare it
to the sampled baselines for the paired diff:

```bash
lex simulate -l 5 --lang english -o /tmp/full.json
lex compare benchmarks/english-5.json /tmp/full.json
```

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
