# Generic answer model; curated lists enter as Profiles

The solver never encodes a particular game's word lists. The default Answer prior is frequency-derived from the Corpus for every language and word length (the Corpus track). Curated lists such as Wordle's answer and allowed-guess lists enter as data, through a Profile: an Answer list (uniform prior) paired with a Guess pool. The Wordle profiles are first-class benchmark tracks because the published optimum for them is a yardstick outsiders recognise; nothing in the solver may branch on which Profile is loaded.

## Considered Options

- Curated lists as evaluation overlays only (targets drawn from them, solver unaware): the earlier position, superseded; it makes the published Wordle optimum incomparable, since the solver still plays against the whole Corpus.
