# lex

THE Wordle Solver

## Getting Started

To run the solver, simply execute the `main` function in `src/main.rs`. This will run the solver against all words in the Knuth five-letter word dictionary and print out the number of guesses it takes to solve each word, as well as the average number of guesses across all words.

## Development

Before making any changes, please ensure you have Rust installed on your machine. You can run the tests using `cargo test` to ensure that all functionality is working as expected. Before commiting any changes, please ensure to install `precommit` and run `precommit install` to set up the pre-commit hooks. This will help maintain code quality and consistency across the project.

## Data

The main data source used for analysis was the Knuth five-letter word dictionary.
This list contains 5678 five-letter words and their associated frequencies in from a few sampling strategies done by Don Knuth and his team.
