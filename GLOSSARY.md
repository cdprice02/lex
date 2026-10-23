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

**Decision tree**:
The shared record of every game position reached so far, where each position is identified by the sequence of (guess, feedback) steps that led to it and remembers the guess the solver chose there.
_Avoid_: Cache (reserved for the on-disk Corpus form), memo table

**Guess path**:
The sequence of (guess, feedback) steps from the start of a game to one position in the Decision tree; two games with the same Guess path have the same Candidate set.
_Avoid_: History (that is the list of guesses a single game played)
