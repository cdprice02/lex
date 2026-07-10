# 02 — Measurement & provenance

Depends on: nothing (00/01 recommended first)
Unblocks: 04, 05, 08 (all solver-behavior changes are gated on this)

## Why

We currently cannot answer "did a change help?":

- `lex-cli/src/simulate.rs` picks targets via
  `word_set.words().cycle().take(num_games)` — words are frequency-descending,
  so `-n 100` simulates the 100 most common words. Average guesses is biased
  low and shifts whenever the corpus pipeline changes.
- The only output is a single average via `log::info!`. No guess histogram,
  and no failure rate — real Wordle fails at 7+, but everything ≥6 is labeled
  "Phew" and play continues unbounded. A change that lowers the average while
  fattening the 7+ tail would look like a win.
- KAIKKI Wiktionary extracts are *rolling*: the same URL yields different
  words over time. Without recorded corpus identity, two runs are not
  comparable and a future "regression" could just be Wiktionary editors
  adding words. (The ngrams snapshot is frozen — `20200217` — but record it
  too.)

## Requirements

- Target selection: seeded uniform sampling (`--seed`) over the dictionary,
  plus a `--targets <file>` override (one word per line). Curated lists (e.g.
  NYT answers) are *optional overlays* for English-5 comparability — never a
  structural requirement (generic-solver constraint).
- `--output json <path>` (or stdout): per-target word and guess count (guess
  sequences optional), full run parameters, corpus identity, timestamp.
- Summary stats: guess-count histogram, failure rate (guesses > 6), mean —
  reported under both uniform weighting and the answer prior.
- Provenance stamping in `DictMetadata` (`lex-data/src/wiktionary.rs`):
  `fetched_at`, source URL, content checksum (e.g. SHA-256 of the downloaded
  file). Benchmark output must embed the checksum it ran against.
- Committed baselines under `benchmarks/` for at least English-5, plus a
  second (lang, N) pair to keep the generic path honest.
- Golden regression test: a small fixed dictionary with an asserted exact
  guess *sequence*. Build the fixture from a file or sorted input — not
  `from_frequency_map`, whose tie order inherits HashMap iteration order.

## Constraints

- **Zero solver-behavior change.** Verify: a fixed target list must produce
  identical guess sequences before and after this refactor.

## Open questions

- JSON schema details (flat records vs. nested summary) — propose one.
- Baseline scope: full-dictionary runs are O(k²) per suggest until task 04
  lands, which may be too slow for English-5. A large fixed seeded sample is
  an acceptable interim baseline; upgrade to full-dictionary (exact,
  zero-variance) after 04. Confirm the interim size with the user.
- Where the human-readable summary goes (stdout vs. log) — currently
  everything is `log::info!`.

## Out of scope

- Fixing the target-distribution/prior mismatch by *changing the prior* —
  that's task 05. Here we only measure under both weightings.

## Done when

Baselines are committed, the golden test is in CI, and a solver change can be
diffed per-target (paired comparison) against the baselines.
