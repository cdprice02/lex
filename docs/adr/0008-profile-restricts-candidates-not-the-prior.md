# A Profile restricts the Candidate set; the solver prior stays frequency-derived

A Profile supplies an Answer list and a Guess pool, and nothing else: the Candidate set starts as the Answer list, but the Answer prior over it is still the Corpus-frequency softmax at the Strategy's temperature, so a Profile run loads the Corpus. The score is weighted by a separate, fixed Report prior (uniform for the Wordle headline, as the published optimum assumes), so the solver's prior can differ from the one that grades it. Profile words the Corpus lacks take the Corpus's minimum observed count (the Unseen-word floor). This supersedes the "(uniform prior)" clause of ADR 0004.

## Considered Options

- A uniform solver prior on Profiles: rejected as fixed behavior. It matches the published optimum's assumption, but temperature already spans it (very high T approaches uniform), a Profile-specific prior would be a branch on the Profile that ADR 0004 forbids, and the benchmark rule in ADR 0007 decides the default temperature rather than the prior's provenance.
- Dropping Profile words the Corpus lacks: rejected; it makes the Guess pool differ from the published 12,972-word pool and the optimum incomparable.
