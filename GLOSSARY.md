# lex

A Wordle solver that picks guesses by Shannon entropy over a frequency-weighted dictionary of words, for any supported language and word length.

## Language

**Corpus**:
The frequency-ranked list of valid words for one language at one word length, built from Google Books ngrams filtered by a Wiktionary dictionary.
_Avoid_: Word list, dataset

**Cache**:
The on-disk form of a Corpus, one file per language and word length, rebuilt rather than edited.
_Avoid_: Store, database

**Corpus checksum**:
The SHA-256 of a Cache's word records, the identity that benchmark reports embed to show two runs used the same Corpus.
_Avoid_: File hash, ngrams hash

**Legacy cache**:
A Cache written before the format carried a header; it is rejected and must be rebuilt, never read.
_Avoid_: Old cache, v0

**Provenance**:
The recorded origin of a dictionary download: source URL, fetch time, and checksum of what was fetched.
_Avoid_: Metadata, source info

**Baseline**:
A committed benchmark report that solver changes are compared against, per target word.
_Avoid_: Golden file, reference run

**Candidate set**:
The words still consistent with every guess played so far in one game.
_Avoid_: Remaining words, dictionary (mid-game)

**Guess pool**:
The words the solver may play as a guess, whether or not they can still be the answer: the Corpus, or a Profile's guess list. The Candidate set is always a subset of it.
_Avoid_: Guess list, allowed words

**Answer prior**:
The probability that each word is the answer, renormalized over the Candidate set as it shrinks: derived from corpus frequency and a temperature, over the Answer list when a Profile supplies one and over the Corpus otherwise. The report prior used to score a run is separate and fixed: uniform on a Wordle Profile.
_Avoid_: Weights, frequency (that is the raw count)

**Report prior**:
The fixed prior that weights a run's results, independent of the solver's own temperature: uniform over the Answer list for the Wordle headline score, frequency-derived at the canonical temperature for the secondary readout.
_Avoid_: Evaluation prior, Answer prior (that is the solver's)

**Unseen-word floor**:
The Corpus's minimum observed count, assigned to Profile words the Corpus lacks so they carry a positive frequency and sort after every Corpus word.
_Avoid_: Default frequency

**Supported length**:
A word length at which a language's Corpus holds at least 500 words; a language's maximum length is its largest Supported length.
_Avoid_: Max length (as a fixed constant)

**Answer list**:
A curated list of the words that can be the answer; when one is set, the Candidate set starts as exactly that list.
_Avoid_: Target list (targets are the words a simulation plays, possibly a subset)

**Profile**:
A named pairing of an Answer list and a Guess pool for one language and word length, such as the Wordle profile for English-5; without one, the Corpus serves as both.
_Avoid_: Mode, preset

**Corpus track**:
Benchmarking with no Profile: answers drawn from the Corpus under the frequency-derived Answer prior.
_Avoid_: Default mode

**Strategy**:
The solver's tunable choices: which Guess pool it draws from (hard mode restricts it to the Candidate set), the Answer prior's temperature, and how much a chance of winning outright adds to a guess's score.
_Avoid_: Policy, config

**Hard mode**:
A Strategy whose Guess pool is only the current Candidate set.
_Avoid_: Constrained mode

**Decision tree**:
The shared record of every game position reached so far, where each position is identified by the sequence of (guess, feedback) steps that led to it and remembers the guess the solver chose there.
_Avoid_: Cache (reserved for the on-disk Corpus form), memo table

**Endgame**:
A position whose Candidate set is at or below the Strategy's Endgame threshold, where the solver searches the game tree exactly instead of scoring guesses by entropy.
_Avoid_: Late game, exact mode

**Endgame threshold**:
The Candidate set size at or below which a position is an Endgame; zero turns the Endgame off.
_Avoid_: Cutoff, search limit

**Guess path**:
The sequence of (guess, feedback) steps from the start of a game to one position in the Decision tree; two games with the same Guess path have the same Candidate set.
_Avoid_: History (that is the list of guesses a single game played)
