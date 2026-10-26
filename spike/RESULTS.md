# Techniques spike: results

Throwaway spike for "Spike: rank search and scoring techniques on the Wordle profile". Never merged.
`spike/src/main.rs` walks the whole policy tree for each configuration; every row is single-threaded,
full target set unless noted.

## Oracle

`spike eng5` (default strategy) matches `lex simulate -l 5 --lang english` on all 27,524 targets
(0 differ): uniform 4.3442, prior-weighted 4.1769, failure 0.37%. `pattern()` agrees with
`WordCorrectness::correct` on a sampled grid of both tracks (`--check`). The endgame 10 row
reproduces `benchmarks/experiments/endgame.md` exactly.

## Wordle answers profile (2,315 answers, 12,972 guesses)

Uniform mean over the answers (the Profile score). Published: optimum 3.4212 (SALET), greedy
entropy 3.4955 and bins 3.4600 with answer-only guesses. Solver prior: frequency over the answers at
T=5, or near-uniform (T=1e6). Every row has 0 failures. Wall times are 1.5 to 20 s, so no row is
time-bound on this track.

| Technique | T=5 | T=1e6 |
|---|---|---|
| Default (entropy + 1.0 P(win)) | 3.4721 | 3.4635 |
| Entropy alone (win weight 0) | 3.4829 | 3.4752 |
| Blend 0.25 H + log2 bins | 3.4376 | 3.4359 |
| Blend 0.5 | 3.4350 | 3.4354 |
| Blend 0.75 | 3.4333 | 3.4320 |
| Bins first, entropy tie-break | 3.4333 | 3.4320 |
| Renyi-2 (expected size) | 3.4873 | 3.4842 |
| Fitted value function | 3.4471 | 3.4337 |
| Endgame 5 / 10 / 15 / 20 (plain) | 3.4778 / 3.4834 / 3.4851 / 3.4825 | 3.4635 / 3.4626 / 3.4609 / 3.4605 |
| Rollout K=10, 12 to 300 candidates | 3.4661 | 3.4553 |
| Rollout K=10, at least 300 | | 3.4320 |
| Rollout K=10, at least 50 | | 3.4263 |
| Rollout K=10, every set | 3.4384 | **3.4212** (SALET, max 5, 17.5 s) |
| Rollout K=3, every set | 3.4397 | 3.4553 |
| Value function + rollout 12 to 300 | 3.4467 | 3.4246 |
| Blend 0.75 + rollout every set | | 3.4212 |

## English-5 Corpus track (27,524 targets, full dictionary)

Report prior T=5. Deltas against the default row.

| Technique | Prior-weighted | Uniform | Failure | Time |
|---|---|---|---|---|
| Default | 4.1769 | 4.3442 | 0.37% | 97 s (lex: 73 s) |
| Blend 0.5 | 4.1797 (+0.0028) | 4.3209 (-0.0233) | 0.40% | 83 s |
| Blend 0.75 | 4.1892 (+0.0123) | 4.3194 (-0.0248) | 0.39% | 86 s |
| Bins first | 4.2524 (+0.0755) | 4.3389 (-0.0053) | 0.43% | 86 s |
| Fitted value function | 4.1494 (-0.0275) | 4.3781 (+0.0339) | 1.26% | 92 s + 85 s fit |
| Endgame 10, plain (phase 08) | 4.1562 (-0.0207) | 4.3699 (+0.0257) | 1.08% | 122 s |
| Rollout K=10, 12 to 300 | 4.1569 (-0.0200) | 4.3422 (-0.0020) | 0.35% | 281 s |
| Rollout K=10, at least 300 | 4.1664 (-0.0105) | 4.3369 (-0.0073) | 0.33% | 753 s (load avg 8 on 4 cores) |

## Guess-pool dedupe (exact)

Distinct labelled partitions as a share of guesses scanned, and non-candidates that do not split
the set, by depth (default strategy):

| Depth | Wordle distinct | Wordle split-less | English-5 distinct (500 sampled) | English-5 split-less |
|---|---|---|---|---|
| 0 | 100% | 0% | 100% | 0% |
| 1 | 44.7% | 14.6% | 76.5% | 3.3% |
| 2 | 7.5% | 48.9% | 26.9% | 19.9% |
| 3 | 3.0% | 70.9% | 5.7% | 53.0% |
| 4+ | | | 2.0% | 76.0% |

## Notes

- The value function is a 0.25-bit table of mass-weighted mean remaining guesses against set
  entropy, fitted on the default policy's own tree for the same track (in-sample; a load-time fit
  would use a sample).
- Rollout cost is `1 + sum_b p_b * base_cost(child_b)` over the top K by the base score, with the
  base policy's subtree cost memoized by candidate set.
- The survey's "2 - w_max" floor is already the per-group floor in `endgame.rs` (`2 * mass - top`):
  only one answer of a group can be finished by its first guess, so the bound holds by construction.
- Only English was measured; the headline tier also needs every other language at length 5.
