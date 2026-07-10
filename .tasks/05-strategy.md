# 05 — Strategy generalization

Depends on: 02 (gating baselines), 04 (affordability)
Unblocks: 08 (endgame search plugs into the strategy abstraction)

## Why

`Guesser::suggest` (lex-core/src/guesser.rs) only considers guesses from the
*remaining candidate set* — Wordle "hard mode". An unconstrained solver
scores every legal word against the remaining candidates and will guess a
known-wrong word to split the field. The classic failure this fixes: after
narrowing to `batch/catch/hatch/latch/match/patch/watch`, hard mode probes
linearly (up to 7 guesses); one disambiguating probe word resolves it in 2–3.
Published results suggest roughly 0.2–0.4 average-guess improvement on
English-5. Pure entropy also ignores that guessing a candidate might *win*
now, and the answer prior currently lives in the wrong crate.

This is one coherent refactor — all pieces touch `Guesser`'s constructor and
hot loop. Do not split it into sequential API churn.

## Requirements

- Extract a strategy abstraction. Prior discussion favored "scoring function
  + guess-pool policy" over a heavyweight trait, but the shape is open —
  propose one after reading the code.
- Two-set design: an immutable guess pool and a shrinking candidate set.
  `push_guess` filters only the candidates; `suggest` scores every word in
  the pool against the candidate distribution.
- Tie-break equal scores in favor of words still in the candidate set, and
  add a win-probability term (e.g. score = entropy + λ·P(guess is the
  answer)).
- Move the answer prior (temperature softmax, currently
  `WordSet::compute_probs` in lex-data/src/word.rs) into lex-core. `WordSet`
  exposes raw frequencies; prior construction is a solver modeling choice.
  Note `retain` currently recomputes the softmax — that responsibility moves
  too; decide where renormalization lives in the new design.
- Make temperature a tunable parameter (CLI-exposed or strategy-config) and
  sweep it against the task-02 baselines; record the result and either
  justify or replace the current `5.0`.
- Answer model stays generic (ROADMAP decision): the frequency-derived prior
  is canonical for every (lang, N); curated answer lists remain optional
  evaluation overlays only.

## Open questions

- λ for the win-probability term: analytic (fold the winning pattern's
  expected saving into the score) vs. empirically tuned — propose.
- Whether the guess pool is the full corpus for the (lang, N) pair or the
  (possibly `--dictionary-length`-limited) loaded set — this changes cost
  and strength; measure both if cheap.
- Hard mode as a retained option (`--hard-mode`)? It's the current behavior
  and nearly free to keep behind the strategy abstraction.

## Out of scope

- Endgame exact search (task 08).
- Micro-optimization of the hot loop (task 06) — but expect suggest cost to
  rise from O(k²) to O(K·k); memoization (04) is what keeps this affordable.

## Done when

Paired per-target comparison against the pre-change baselines shows the
expected improvement with no failure-rate regression; new baselines are
committed; temperature sweep results are recorded.
