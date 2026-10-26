# The Opening book caches the online policy; budgets are timed warm

Where the top of the Decision tree is too slow to search per move, its guesses are precomputed into an Opening book under `data/` (ADR 0003) and looked up. The book is a cache of the online policy, never a stronger policy: it stores only guesses the online search would choose, so a game plays bit-identically with or without it and a lookup miss computes the position online. Its key is everything that can change a chosen guess (pool identity through the Corpus and Profile checksums, word length, every Strategy field, and the solver-source digest), so any solver change rebuilds it. It holds every position on the policy's path to any Answer of the Track's full Answer set whose Candidate set is at or above one global floor, and every Track gets one through the same code path. Because building the book costs what a cold run of those top levels costs, the run and assist budgets in ADR 0007 are measured with the book already built; building it has its own budget of 60 minutes on one core per Track, reported beside the run time. Assist's per-move budget holds on the book's path; a guess off that path is searched online.

## Considered Options

- An exact solve as the book: rejected; the published one took days on 64 cores, and top-K rollout already reaches the 3.4212 optimum on the Wordle profile.
- A stronger offline policy than the online one: rejected; results would depend on whether the file exists, a partial book would need its own handoff rule, and the spike showed rollout alone captures the gain.
- Timing runs cold: rejected; rollout at every set on English-5 cannot fit the 10-minute budget without the book.
- Booking every guess at early positions so off-path assist moves stay fast: rejected; it multiplies the build by the Guess pool size for moves the solver did not advise.
