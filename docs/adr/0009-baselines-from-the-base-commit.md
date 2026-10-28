# Baselines are reruns of the base commit; a PR carries only its Verdict

Superseded by ADR 0012.

No benchmark report is committed as a baseline. Every Verdict pairs the candidate against a run of the base commit, built locally and played on the same local Corpus, with that run cached by commit, Corpus checksum, and Suite manifest. The Corpus rolls with every Wiktionary extract, so a committed report goes stale as soon as the Corpus is rebuilt, and a full report is too large to commit; a rerun of the base on identical data keeps every comparison paired with nothing stale. The PR commits a small verdict file (per-Track deltas, provenance, and a digest of the solver source, never words), which CI checks without running the suite, since CI cannot afford the Corpus download and its runners are too noisy to time on.

## Considered Options

- Committed sampled baselines: rejected; they went stale on the first Corpus rebuild and cover only the tracks small enough to commit.
- Running the suite in CI: rejected; each run would stream the full shard set, and timing on shared runners would move the Tier boundaries by noise.
