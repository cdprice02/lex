# Baselines

Committed `lex simulate` reports that solver changes are measured against. Each is
a sample, not the full dictionary: full-dictionary runs are quadratic per suggest until
`.tasks/04-memoization.md` lands, after which these should be regenerated as exact runs.

| File | Command |
|---|---|
| `english-5.json` | `lex simulate -l 5 --lang english -n 2000 --seed 0 -o benchmarks/english-5.json` |
| `french-5.json` | `lex simulate -l 5 --lang french -n 500 --seed 0 -o benchmarks/french-5.json` |

The French baseline keeps the generic (language, length) path honest.

## Comparing a change

Rerun the same command to a scratch path, then:

```bash
lex compare benchmarks/english-5.json /tmp/candidate.json
```

Targets are paired by word, so the comparison is not affected by sampling noise. It warns when
the corpus checksums differ.

## Provenance

KAIKKI Wiktionary extracts are rolling. A report's `corpus.ngrams_sha256` identifies the exact
word list and frequencies it ran against; `corpus.dict` records the Wiktionary download.
A rebuilt corpus will usually have a different checksum, so baselines must be regenerated
(and the change noted) when the corpus is rebuilt, not mixed with reports from an older one.
