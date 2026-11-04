# Endgame search results (phase 08)

English-5, full dictionary (27,524 targets), corpus `29104fb5747b`, default strategy (unconstrained,
T=5, win weight 1). Every row is paired by target against the same run with the endgame off
(uniform mean 4.3442, prior-weighted mean 4.1769, failure rate 0.37%, 73 s). Deltas are in guesses,
negative is better. Timings are one sequential run each on an otherwise idle machine.

`--endgame-threshold N` searches the game tree exactly once at most N candidates remain.
`--endgame-aversion L` minimizes the mean of e^(L x guesses) instead of the mean guess count, so a
long game costs more than its length alone; 0 is plain expected guesses.

| Threshold | Aversion | Prior-weighted | Uniform | Failure rate (>6) | Longest game | Time |
|---|---|---|---|---|---|---|
| 5 | 0 | -0.0133 | +0.0123 | 0.93% | 9 | 76 s |
| 10 | 0 | -0.0207 | +0.0257 | 1.08% | 9 | 114 s |
| 15 | 0 | -0.0233 | +0.0284 | 1.07% | 9 | 905 s |
| 10 | 0.25 | -0.0181 | +0.0108 | 0.75% | 9 | 144 s |
| 10 | 0.5 | -0.0121 | +0.0033 | 0.53% | 9 | 121 s |
| 10 | 1 | +0.0033 | -0.0032 | 0.32% | 8 | 140 s |
| 10 | 2 | +0.0268 | -0.0049 | 0.21% | 8 | 144 s |
| 15 | 1 | +0.0019 | -0.0043 | 0.29% | 8 | 1185 s |

At threshold 5 the aversion sweep (0.1, 0.25, 0.5, 1) traces the same line: prior-weighted -0.0131,
-0.0119, -0.0081, +0.0034 and failure rate 0.83%, 0.71%, 0.52%, 0.33%. Its wall times are not
reported because the machine was shared.

## Reading

This verdict was reached when failure rate outranked the mean. Under the current objective
(`docs/adr/0007-benchmark-objective.md`) a lower prior-weighted mean wins, which reopens the
plain-expectation settings.


- The endgame does not pay for itself on English-5. The best prior-weighted gain is 0.02 guesses
  (0.5%), and every setting that gets it worsens the uniform mean and more than doubles the
  failure rate, because exact expected play gives up rare words. Aversion trades the gain back for
  a lighter tail; no setting improves all three metrics.
- The trap-word families the brief targeted (`-atch`, `-ight`, 31 targets) do not improve: mean
  4.387 guesses with the endgame off, 4.419 at threshold 10 with aversion 1. Phase 05's
  unconstrained probing already resolves most of them.
- Returns diminish after threshold 10: 5 to 10 adds 0.0074 prior-weighted, 10 to 15 adds 0.0026 at
  eight times the time.
- The endgame stays off by default (`endgame_threshold = 0`), so the committed baselines and the
  default behavior are unchanged. Other languages and lengths were not measured.

## Search cost

Guesses at each set are tried in order of a cheap lower bound on their cost, and the scan stops once
the bound exceeds the best cost found, so most of the pool is rejected without recursing. The
ordering is exact: it reproduced the earlier unordered search with 0 of 27,524 targets changed at
thresholds 5 and 10. Threshold 10 went from 1313 s to 114 s, and threshold 15 became feasible.
Limiting probes to the top K by entropy was tried and rejected: scoring the pool by entropy at every
set cost more than the exact search it was meant to shorten (threshold 5 with K=50 exceeded 25
minutes against 201 s).

## Reproducing

```bash
lex simulate -l 5 --lang english -o /tmp/off.json
lex simulate -l 5 --lang english --endgame-threshold 10 --endgame-aversion 1 -o /tmp/on.json
lex compare /tmp/off.json /tmp/on.json
```

Rebuilding the corpus changes its checksum; rerun the endgame-off run rather than comparing against
an older report. The search's predicted cost is checked against simulated games on real Endgame
roots by `cargo test -p lex-core --release -- --ignored real_corpus`.

## As the default, with rollout

The Endgame at threshold 10 with plain expectation became the default Strategy alongside Rollout
(K=10). Every Track was regenerated against the rollout-only snapshots; the Verdict accepts with a
headline average delta of -0.0106 and an average failure-rate delta of +0.0019.

| Track | Prior-weighted | Uniform | Failure rate (>6) | Warm run |
|---|---|---|---|---|
| english-5 | 4.1572 to 4.1440 | 4.3481 to 4.3664 | 0.42% to 0.95% | 300 s |
| french-5 | 4.0489 to 4.0360 | 4.2330 to 4.2441 | 0.46% to 0.84% | 127 s |
| german-5 | 3.9083 to 3.8964 | 4.0432 to 4.0560 | 0.08% to 0.25% | 37 s |
| spanish-5 | 4.0953 to 4.0806 | 4.2861 to 4.3042 | 1.09% to 1.85% | 158 s |
| italian-5 | 3.6610 to 3.6503 | 3.7956 to 3.8055 | 0.03% to 0.11% | 12 s |
| russian-5 | 4.4507 to 4.4367 | 4.5929 to 4.6049 | 1.08% to 1.73% | 697 s |
| wordle | 3.3757 to 3.3671 | 3.4691 to 3.4721 | 0.00% to 0.00% | 6 s |

- Every length-5 Corpus Track gains about 0.01 to 0.015 prior-weighted and loses about the same
  uniformly, with failure rates up by 0.1 to 0.8 points: plain expectation spends rare words' games
  to shorten common ones, as in the threshold table above. The aversion rows show the knob that
  trades this back.
- Wordle is judged on the uniform mean and moves +0.0030, inside the 0.005 headline margin.
- The six length-5 headline Tracks total 1,331 s warm; Russian-5 alone exceeds the 600 s budget.
- The slowest Opening book is Russian-9 at 86.8 minutes, inside the 2-hour gate.
- Speed bar, English-5 full simulate, 3 alternating rounds: rollout only 167.1 s (166.7 to 167.7),
  with the Endgame 291.1 s (290.4 to 291.7), 1.74 times the time.
- The rollout multiplier stays 8: rollout over greedy measured 8.35 (geometric mean of 12, 60 and
  300 candidates) with the Endgame as the base policy, against 8.1 with greedy.
