# Benchmark objective: mean guesses first, across every language

A solver change is judged by mean guesses first and failure rate (more than 6 guesses, at every word length) second. The score is the uniform mean on a Wordle profile and the prior-weighted mean on the Corpus track; failure rate breaks ties within 0.001 guesses and never vetoes a lower mean. Because lex is language agnostic, the score is pooled over a benchmark matrix of every language at every Supported length, in three tiers:

- headline, full dictionary: the Wordle answers profile and every language at length 5, with the Wordle guess-set profile and Wordle hard mode run alongside but reported, never judged;
- breadth, paired by word: every language at lengths 4 up to the largest whose run fits the time budget, on the full dictionary where that fits and 500 sampled targets otherwise;
- smoke, must finish: length 3 and the remaining Supported lengths.

A change lands when the average per-track mean delta over the headline tier is negative, no headline track regresses by more than 0.005 guesses, and no breadth track regresses by more than 0.02. Tunables are derived from the Corpus at load time or set once globally against the pooled matrix; a table of values tuned per language and length is not allowed.

Budgets: a full-dictionary run at length 5 finishes within 10 minutes on one core, breadth runs within 10 minutes each, assist within 1 second per move at every length, and in-memory precomputed structures within 1 GB. Per-thread scratch reused across calls (pattern buckets, sort buffers) is working memory, not a precomputed structure, and does not count toward that cap. Run and assist times are measured with the Opening book built; building it has its own budget (ADR 0011).

## Considered Options

- Failure rate first, never traded for mean: rejected; fast solving is the goal, and a lower mean mostly lowers failures with it. Under the earlier rule the phase-08 endgame was rejected for doubling failures at a 0.02 prior-weighted gain.
- English-5 as the only scored track: rejected; it invites tuning to one corpus, the opposite of the project's point.
