# lex-core

Entropy-based Wordle solver library. Given a `WordSet`, suggests guesses that maximize Shannon entropy over the remaining candidate set and filters candidates after each guess.

## Public API

```rust
use lex_core::{Guesser, Guess, WordCorrectness, play};
use lex_data::WordSet;

// Create a guesser and get the first suggestion
let mut guesser = Guesser::new(word_set);
let suggestion = guesser.suggest().unwrap();

// Apply feedback and narrow the candidate set
let feedback = WordCorrectness::correct(target, suggestion);
guesser.push_guess(Guess::new(suggestion, feedback));

// Or run a full game and get the result
let result = play(target, &word_set, vec![])?;
println!("{} guesses", result.num_guesses());
for guess in result.guesses() {
    println!("{} -> {}", guess.word(), guess.correctness());
}
```

## Key types

| Type | Description |
|---|---|
| `Guesser<N>` | Holds the current `WordSet` and guess history; `suggest()` + `push_guess()` |
| `Guess<N>` | A word + its `WordCorrectness` feedback |
| `WordCorrectness<N>` | Array of `Correctness` values for each position |
| `Correctness` | `Absent` / `Misplaced` / `Correct` |
| `GameResult<N>` | Returned by `play()`; exposes `word()`, `guesses()`, `num_guesses()` |
