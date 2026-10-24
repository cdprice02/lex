# Survey of Wordle solving techniques

Research for issue #10 (map #8). Question: which techniques beat one-ply entropy, by how much, at
what cost, and which survive lex's constraints (any language and length, pools up to ~150k words,
deterministic, 1 GB cap on precomputed structures, mean guesses first per ADR 0007).

Conventions: **A** = the original 2,315 Wordle answers, **G** = the original 12,972 allowed
guesses. "Uniform" means every answer equally likely. Numbers are from the cited source; "(derived)"
marks my arithmetic on cited numbers and "(unsourced)" marks my own reasoning. Several primary pages
were unreachable (Selby's blog returned 404, Olson's page rendered empty, Glaiel's post and
3Blue1Brown's second-video score were not retrieved); those figures are marked low confidence.

## Bottom line

1. Nothing published is measured on a frequency-weighted answer set of 27k to 150k words. Every
   published mean below is on 2.3k to 3.2k curated answers, so our 4.177 prior-weighted and 4.344
   uniform are not comparable to any of them. Gains below are therefore priors for the spike, not
   forecasts.
2. The gap between one-ply entropy and the proven optimum on the original lists is small: about
   +0.04 to +0.09 guesses (1.1% to 2.5%). That bounds what any non-exact technique can win there.
3. Exact solving is out of reach as a general method (days on 64 cores for 2,315 answers) but its
   pieces (lower bounds, split-only guesses, memoization on the candidate set) are exactly what the
   existing Endgame already uses.
4. The cheapest credible gains are a different one-ply score (number of bins, or an expected
   remaining guesses value function) and a narrow top-K one-step rollout. Ranked shortlist in the
   last section.

## 1. Exact solving

| Result | Lists | Mean | Max | Cost | Source |
|---|---|---|---|---|---|
| Proven optimum, opener SALET | A / G | 3.42117 (7,920 total) | 5 | days, 64 cores (Bertsimas & Paskov); Selby: "a day or two on single core" for a full proof | [Cunanan & Thielscher, IJCAI 2023](https://ar5iv.labs.arxiv.org/html/2305.09111), [Selby](https://github.com/alex1770/wordle), [Bertsimas & Paskov, Oper. Res. 73(3), 2025](https://ideas.repec.org/a/inm/oropre/v73y2025i3p1384-1394.html) |
| Proven optimum, opener TARSE | 3,158 answers / 14,885 guesses | 3.5526 | 6 | "days, some parallel" | Selby part 3 via [Greenberg, arXiv 2408.11730](https://www.arxiv.org/pdf/2408.11730) |
| Optimal hard mode | A / G | 3.5085 | 6 | not stated | Selby via [Gigazine](https://gigazine.net/gsc_news/en/20220129-the-best-strategies-for-wordle) (secondary) |
| Optimal "superhard" mode, opener SCAMP | A | 3.7162 | 5 | about three weeks, no strong pruning | [Greenberg](https://www.arxiv.org/pdf/2408.11730) |

Bertsimas & Paskov (BP) state their guess pool as 10,657 words; Poirrier calls 10,657 the
non-answer words only, so the pool size in BP is ambiguous
([BP paper](https://auction-upload-files.s3.amazonaws.com/Wordle_Paper_Final.pdf),
[Poirrier](https://www.poirrier.ca/notes/wordle/)). The often-quoted 3.4201 was not found in any
fetchable source (it may be 7,897/2,309 on the NYT list, unverified).

**How the exact solves work.** BP recurse from the root with memoization on the candidate set
(state space about 2^2315), using a precomputed feedback table of |G| x 243 x 2,315 = 5.995e9
booleans. Pruning, all exact:

- Base cases: one candidate costs 1 guess, two candidates cost exactly 1.5.
- Lower bound (2n-1)/n per state; a guess is dropped once its partial sum exceeds the best found.
- Try guesses that are candidates first, and expand the largest-probability bucket first.
- Cunanan & Thielscher: only guesses that split the set into at least two groups matter; a chain of
  lower bounds V1..V6 narrows the 12,972 root guesses to 12,453, 1,711, 324, 138, 1, 1 once an upper
  bound is known (Table 4).
- Bounded breadth is not exact but nearly: breadth 1 gives 7,944 total, breadth 5 gives 7,921,
  breadth 10 and 20 give 7,920 (the optimum) on A / G.

Dominance, symmetry and canonical-form pruning have no Wordle source. Poirrier's depth-limited work
optimizes worst-case depth, not the mean (his NYT tree has mean 4.2361), so it does not transfer.
Deciding whether a winning Wordle strategy exists is NP-complete
([arXiv 2204.04104](https://arxiv.org/pdf/2204.04104), title only).

**Scaling.** Feedback tables: 12,972^2 bytes is 160 MB (Poirrier); 27.5k^2 is 0.76 GB at one byte
per cell (length 5 only, since 243 patterns fit a byte) and 1.5 GB at u16 (derived); 150k^2 is 22.5
GB at u8 and 45 GB at u16 (derived). Olson's serialized solution trees run about 6 GB
([Olson](https://jonathanolson.net/experiments/optimal-wordle-solutions)). No exact solve exists
for a 10k+ answer set, for other languages, or for other lengths. Cunanan & Thielscher ran
breadth-20 searches on Nerdle (17,723 words) and Primel (8,363 words) without proofs.

**Verdict for lex.** A full exact tree is infeasible at our sizes and budgets. The transferable
parts are already in `endgame.rs` (lower-bound ordering, set-keyed memo). One unused piece: with
weights summing to 1, a set's expected cost is at least 2 - w_max (hit the heaviest word first,
every other word needs one more), the frequency-weighted analogue of BP's bound (unsourced).

## 2. Value functions and one-ply scores

**One-ply scores**, greedy, guesses restricted to answers
([Greenberg, arXiv 2408.11730](https://www.arxiv.org/pdf/2408.11730), Table 2):

| Score | Mean on A | Mean on 3,158-word list | Gap to optimum on A |
|---|---|---|---|
| Most parts (number of bins) | 3.4600 | 3.6089 | +0.039 (derived) |
| Entropy | 3.4955 | 3.6431 | +0.074 (derived) |
| Expected bin size (sum n_i^2) | 3.5210 | 3.6602 | +0.100 (derived) |
| Minimax (largest bin) | 3.5564 | 3.7172 | +0.135 (derived) |

Bins as the primary key with entropy as tie-break gives 3.4553 on A and 3.6067 on the 3,158 list
(same paper). A max-entropy solver using the full G pool reaches 3.476 (opener SOARE;
Greenberg Table 1 citing Luebeck and Das), and Doddle with the full G pool and opener SALET gets
3.432 for entropy and 3.482 for minimax ([PyPI doddle](https://pypi.org/project/doddle/), via the
heuristics notes). So the ranking among one-ply scores is not stable across pools: bins beat entropy
by 0.036 on answer-only guesses, but the full-pool entropy result (3.432) is already within 0.011 of
optimal. Nobody published a ranking on G with weights.

**Value functions.** 3Blue1Brown's published code ([simulations.py](https://github.com/3b1b/videos/blob/master/_2022/wordle/simulations.py))
estimates remaining guesses from remaining entropy H in bits as
f(H) = 2^-H + 2 (1 - 2^-H) + 1.5 H / 11.5, with the constant fitted to "about 11.5 bits is about 3.5
guesses", and scores a guess as p_win + (1 - p_win) (1 + f(H_after)). It keeps the top 250 by
entropy and refines the top 25, and has an optional two-ahead mode. The video's resulting averages
could not be verified; the only third-party figure found is about 4.1 falling to about 3.6 for the
first video ([Beebom](https://beebom.com/puzzle/wordle-bot-explained/), low quality). BP report that
perturbing a value function by 0.1% already moves a policy off optimum, which is why fitted value
functions are best used for ordering and truncation, not as the final judge.

Our current `entropy + lambda * P(win)` is already a cheap form of this. The 3Blue1Brown form
differs by using the entropy remaining after the guess, averaged over buckets, which needs a per
bucket p log p sum, available from the same bucket pass in `guess_entropy`.

## 3. Look-ahead and rollouts

[Wordle rollout paper, arXiv 2211.10298](https://ar5iv.labs.arxiv.org/html/2211.10298)
(A / G, uniform): a max-information-gain base policy scores 3.6108 easy and 3.6078 hard; a one-step
rollout over the top 10 guesses per stage reaches 3.4345 easy (0.4% above optimal) and 3.5231 hard;
top 100 adds little at much higher cost. Gain over its own base: 0.176 easy, 0.085 hard. The base
there is weaker than Doddle's entropy (3.432), so the 0.176 will not carry over: against a strong
entropy base the room is at most the remaining 0.01 to 0.07 to optimal on these lists.
The Cunanan & Thielscher breadth result (breadth 10 reaches the optimum) is the same observation
from the exact side: a narrow top-K per stage is enough.

Cost model: a rollout step at a node with K guesses and b non-trivial buckets runs b child
decisions, each a full scan of the guess pool against a smaller candidate set. Our Endgame notes
already show that scoring the whole pool at every set cost more than the exact search it was meant
to shorten (`benchmarks/experiments/endgame.md`), so any look-ahead must use the memoized Decision
tree and small K, and must avoid full-pool child scans at large sets (use child candidates plus a
shortlist).

## 4. Learned solvers

Reinforcement-learning and neural solvers trail by roughly 0.2 to 1.2 guesses and depend on a fixed
vocabulary: teacher-guided DQN 3.678 and pure DQN 4.582 on an unreviewed write-up
([HF space](https://jhffmn82-wordle-ml.hf.space/)); a deep-RL blog reports about 3.9 to 4.1
([W&B report](https://wandb.ai/andrewkho/wordle-solver/reports/Solving-Wordle-with-Reinforcement-Learning--VmlldzoxNTUzOTc4));
the rollout paper says Deep-Q and A2C did "considerably worse" than its base. No credible 2023 to
2026 RL result beats the heuristics. Not language agnostic without retraining; rejected.

## 5. Guess-pool reduction

- Exact: drop guesses that give one group at the current set; drop guesses whose lower bound exceeds
  the incumbent; among guesses with identical partitions keep one, preferring a candidate (the last
  two rules are standard but unsourced for Wordle). The Endgame already applies the first.
  No published count of dominated or duplicate-partition guesses exists, so the fraction removable
  from a 27k to 150k pool is unknown and must be measured (cheap: hash each guess's partition).
- Near-exact: top-N per stage. Selby's `-n10` lands on SALET at 7,920 and `-n100` is "very accurate";
  3Blue1Brown keeps the top 250 by entropy ([Selby](https://github.com/alex1770/wordle)). The
  catch for lex is that ranking the pool by entropy costs a full pool scan, the very cost to avoid.
- Restricting guesses to candidates loses little at the margin
  ([Greenberg](https://www.arxiv.org/pdf/2408.11730)), but our own phase 05 shows unconstrained
  guessing lowered the prior-weighted mean by 0.458 versus hard mode (`benchmarks/experiments/strategy.md`),
  so pool restriction is a speed lever only where measured to be free.

## 6. Scaling and memory at 150k words

All sizes derived from cell counts: a full pattern matrix is 22.5 GB (u8, length 5 only) or 45 GB
(u16, lengths 6 to 10) at 150k x 150k, so it cannot be held. Even 27.5k x 27.5k is 0.76 GB at u8
and 1.5 GB at u16, near or over the cap. Only about 3,300 candidates x 150k guesses fit in 1 GB at
u16. Consequences: feedback must be computed against the live candidate set (as `guess_entropy`
does now, O(pool x candidates) per node), and any precomputed artifact must be small: the opener and
second-level choices per feedback pattern are the natural candidates, cached under `data/` and
keyed by Corpus checksum and Strategy (ADR 0003 allows rebuilt derived artifacts). A 150k x 150k
first-move scan is 2.25e10 feedback evaluations, so the root needs caching or a shortlist at those
pool sizes. No source quantifies SIMD feedback speed or candidate sampling for entropy estimates;
they are design options only.

## 7. Answer-prior modelling

Wordle's real answer list is curated, so published solvers use uniform priors. 3Blue1Brown weights
words by a sigmoid over frequency rank (n_common = 3000, width 10) in the published code, with no
measured effect reported. BP say their framework supports nonuniform probabilities but I could not
extract numbers. No published study measures a frequency-derived prior on mean guesses or on
rare-word failure. Non-English and long-word evidence is thin: a greedy English solver restricted to
words without repeated letters averages 8.66 tries at length 3, 4.84 at length 5, 2.80 at length 9
([arXiv 2305.14756](https://ar5iv.labs.arxiv.org/html/2305.14756), via the pools notes); a Spanish
test reached 88% success ([arXiv 2510.02855](https://arxiv.org/html/2510.02855v1)); no study found
for lengths 10 to 12. Our own temperature by win-weight sweep (`benchmarks/experiments/strategy.md`)
is the only evidence for this regime, and no technique here changes it.

## 8. What our own endgame data adds

Under ADR 0007 (mean first) the endgame verdict in `benchmarks/experiments/endgame.md` reopens:
English-5 corpus track, threshold 10, aversion 0 gives a prior-weighted -0.0207 at 114 s (budget:
10 minutes) with uniform +0.0257 and failure 1.08%. Threshold 15 gives -0.0233 but takes 905 s,
over budget. This is a measured, already-built gain, not a new technique, but its uniform-mean
regression matters on the Wordle profiles, where uniform is the score. The ADR's headline rule
averages per-track deltas, so whether to enable it is a benchmark-matrix decision (it should be
measured on the Wordle profiles and other languages before landing).

## Judgement against our constraints

| Technique | Any language and length | 150k pool | Deterministic | 1 GB | Expected gain | Cost |
|---|---|---|---|---|---|---|
| Full exact DP / B&P | yes | no (days at 2.3k answers) | yes | no | optimum, up to ~0.07 | prohibitive |
| Number-of-bins / expected-size scores | yes | same O(pool x cands) as now | yes | none extra | 0.0 to 0.04 (unweighted, answer-only guesses) | one extra accumulator, no new passes |
| Expected remaining guesses value function | yes (fit per Corpus at load time, ADR allows) | same scan | yes | none | unknown, the 3b1b form plus bucket-entropy average | per-bucket p log p, small fit |
| Top-K one-step rollout with memo | yes | needs K x buckets child scans | yes | tree memo grows | up to 0.01 to 0.07 on a strong base | high at large sets, gate by size |
| Endgame, plain expectation | yes | already built | yes | memo only | -0.02 prior-weighted (measured) | 114 s at T=10, 905 s at T=15 |
| Guess-pool reduction (partition dedupe, split-only) | yes | main speed lever | yes | hash set per node | speed, mean unchanged if exact | one partition hash per guess |
| RL / neural | no | no | depends | model | negative | n/a |

## Ranked shortlist for the spike (#17)

Evaluate on English-5 first, but report the pooled tiers of ADR 0007. As an anchor, also run on A / G
so the result can be set against the proven 3.42117 and Greenberg's table.

1. **Number-of-bins and expected-size one-ply scores, as a score blend.** Gain 0 to 0.04 (best
   published difference: bins 3.4600 vs entropy 3.4955 on A, answer-only guesses; unverified with
   weights or the full pool). Cost: negligible. Sketch: add a `Score` enum to `Strategy`
   (`Entropy`, `Bins`, `ExpectedSize`, `Blend`) and have `guess_entropy` in `candidates.rs` return
   all of `(entropy, bins, sum_p2)` from the one pass over the touched buckets (the touched list
   already gives the bin count for free). `for_each_guess` ranks by the chosen score, keeping the
   win-weight term and the same candidate-first tie-break. Because it is a pure score change inside
   `Candidates::suggest`, the Decision tree memo stays valid; the Strategy becomes part of tree
   identity as it already is. Prior weighting: use the weighted sum of squares (expected remaining
   probability mass) and treat bins as a count of non-empty buckets, with a tie-break on entropy.
2. **Expected-remaining-guesses value function (3Blue1Brown form), fitted per Corpus.** Gain
   unknown; plausible 0.01 to 0.03 beyond the existing win bonus. Cost: low. Sketch: score =
   `p_win * 1 + (1 - p_win) * (1 + f(H_after))` where `H_after = sum_b p_b * H_b` is accumulated in
   the same bucket pass (needs per bucket sum of p and sum of p ln p). `f` has the 3b1b shape with
   one scale constant fitted at load time by regressing simulated remaining guesses on remaining
   entropy over a small sample of targets from the Corpus (allowed by ADR 0007; no per-language
   table). This replaces the hand-set `win_weight`, removing one tunable from the global sweep.
3. **Top-K one-step rollout in the Decision tree.** Gain bounded by the remaining gap to optimal
   (0.01 to 0.07 by the published lists; the rollout paper's 0.176 is against a weak base). Cost:
   high, so gate by candidate-set size (for example 12 to ~300 candidates, between the Endgame and
   the one-ply regime) and K <= 10. Sketch: in `DecisionTree::suggest`, when the set is in the
   gated band, take `suggest_top_k(strategy, K)` (already exists), and for each of the K guesses
   compute the expected cost as `sum_b p_b * (1 + c(child_b))` where `c(child)` is the greedy
   expected remaining cost of the child set, obtained from the child's own memoized one-ply
   suggestion via the value function from item 2 (children are Decision tree nodes, so the memo
   amortizes across games). Deterministic as long as ties break as now. Use item 4's lower bound
   to cut guesses early.
4. **Endgame: enable plain expectation and tighten the bound.** Measured -0.0207 prior-weighted at
   T=10 and 114 s. Cost: zero new code for the enablement; the bound is a few lines. Sketch: set
   `endgame_threshold` to 10 with aversion 0 as the default after running the full matrix on the
   Wordle profiles (uniform regresses +0.0257 on English-5); add the 2 - w_max floor and the
   split-count bound of Cunanan & Thielscher to `search` in `endgame.rs` so a higher threshold (15
   or more) fits the 10-minute budget. Memory is the set-keyed memo only.
5. **Guess-pool reduction as a speed enabler (for #19), not a mean gain.** Exact. Sketch: in
   `Candidates::for_each_guess`, group pool entries by identical partition of the current
   candidate set and score one representative (a candidate if the group has one); skip guesses with
   a single group. Measure first how many are removed at each depth on 27k to 150k pools; the
   literature has no number. Pair with a persisted root and second-level choice per feedback
   pattern under `data/` (checksum-keyed, rebuilt, per ADR 0003) for #18, since a 150k root scan
   cannot meet a 1 s assist budget.

Rejected: full exact DP (cost), pattern-matrix precompute (memory at 150k), RL (not agnostic, worse).

## Source reliability

Primary and read in full or in the relevant part: Cunanan & Thielscher (ar5iv), Greenberg (arXiv),
the rollout paper (ar5iv), BP (author PDF), Selby README, 3Blue1Brown simulations.py. Secondary or
partial: Selby's articles (404), Olson (empty), Gigazine for hard mode, Glaiel (ROATE 3.49417,
secondary summary only, low confidence), HF and W&B RL figures (low quality), 3Blue1Brown video
averages (unverified). Not found: any exact solve beyond 3,158 answers, any non-English or non-5
optimal result, any measurement of frequency priors, any count of dominated guesses, and any source
for SIMD feedback speed.
