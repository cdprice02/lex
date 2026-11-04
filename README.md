# lex

[![lint](https://github.com/cdprice02/lex/actions/workflows/lint.yml/badge.svg?branch=main)](https://github.com/cdprice02/lex/actions/workflows/lint.yml)
[![test](https://github.com/cdprice02/lex/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/cdprice02/lex/actions/workflows/test.yml)
[![docs](https://github.com/cdprice02/lex/actions/workflows/docs.yml/badge.svg?branch=main)](https://github.com/cdprice02/lex/actions/workflows/docs.yml)
[![rustc: nightly](https://img.shields.io/badge/rustc-nightly-orange.svg)](rust-toolchain.toml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An entropy-based Wordle solver. Simulates games against the [Google Books Ngrams V3](https://storage.googleapis.com/books/ngrams/books/datasetsv3.html) corpus, cross-referenced with [Wiktionary](https://kaikki.org) to keep only real dictionary words.

## How it plays

- **Entropy scoring.** Each guess is scored by the Shannon entropy of the feedback it splits the Candidate set into, weighted by a frequency-based Answer prior, plus a bonus for the chance the guess is the answer itself. By default every word in the Corpus may be guessed (`--pool all`).
- **Opening book.** The top levels of the game tree are built once and looked up, so positions too slow to search per move cost nothing at play time. A game plays identically with or without the book.
- **Rollout.** At mid-sized Candidate sets the top 10 guesses (`--rollout-k`) are each played out under the base policy (greedy entropy, then the Endgame once 10 or fewer candidates remain), and the one with the fewest expected guesses wins.
- **Endgame.** At 10 or fewer candidates (`--endgame-threshold`) the solver searches the game tree exactly to minimize expected guesses.

Methodology and the current results table are in [`benchmarks/README.md`](benchmarks/README.md).

## Quick start

Requires nightly Rust. Use whichever toolchain setup you prefer:

```bash
# via rustup
rustup install nightly
rustup override set nightly

# via Nix + direnv (if you use flakes)
direnv allow
```

Then build and run:

```bash
cargo run --release -- simulate -l 5 --lang english -n 100   # batch simulation
cargo run --release -- assist -l 5 --lang english            # assist a live game
```

The first run for a language downloads its Wiktionary word list and all Google Books Ngrams shards, then caches everything to `data/`. Subsequent runs are fully offline.

## CLI

Subcommands: `simulate` (batch games, average guess count), `assist`
(interactive helper for a live Wordle: suggests guesses, you enter the
feedback as letters `gyx`, digits `210`, or pasted emoji), `compare` (paired
comparison of two `simulate --output` reports), `book build` (the opening
book, below) and `clear`.

`simulate` and `assist` build the opening book for their game and strategy on
first use (progress on stderr) and load it afterwards; `--no-book` plays fully
online. The book is a cache of the solver's own choices, so games are identical
either way. `lex book build` takes the same game and strategy flags, builds the
book or reports it up to date (`--force` rebuilds), and prints its size and
build time. Books live in `data/books/`; `lex clear --books [--lang L] [-l N]`
removes them, and clearing a corpus or a Profile removes its books too.

`simulate`, `assist` and `book build` share the game and Strategy flags:

| Flag | Default | Description |
|---|---|---|
| `-l` / `--word-length` | required without `--profile` | Word length (3-21) |
| `--lang` | required without `--profile` | Language corpus (see below) |
| `--profile` | none | Play a Profile: its Answer list and Guess pool, with its language and word length (`wordle`, `wordle-all`) |
| `--dictionary-length` | all | Cap the candidate dictionary size |
| `--data-dir` | `data` | Directory for cached word-frequency files |
| `--pool` | `all` | Words that may be guessed: `all`, `hard` (official Wordle hard-mode rule) or `ultrahard` (only words consistent with every feedback) |
| `--temperature` | `5` | Answer prior temperature; higher flattens the frequency-based prior |
| `--win-weight` | `1` | Score bonus, in bits, per unit of probability that a guess is the answer |
| `--endgame-threshold` | `10` | Search exactly once this few candidates remain (`0` disables) |
| `--endgame-aversion` | `0` | Endgame risk aversion; `0` minimizes plain expected guesses |
| `--rollout-k` | `10` | Top-scored guesses compared by Rollout (`0` disables) |
| `--no-book` | off | (`simulate`, `assist`) Play fully online instead of using the opening book |
| `--force` | off | (`book build`) Rebuild even when the book is up to date |

`simulate` also takes:

| Flag | Default | Description |
|---|---|---|
| `-n` / `--num-games` | every word | Number of target words to sample |
| `--seed` | `0` | Seed for sampling targets (requires `-n`) |
| `--targets` | none | File of target words, one per line, to play instead of sampling |
| `-o` / `--output` | none | Write the full JSON report here (`-` for stdout, which moves the summary to stderr) |

`assist` also takes `-s` / `--suggestions` (default `5`): suggestions shown per turn.

`clear` takes `--lang`, `--profile`, `--books` and `-l`, plus `--data-dir`; see `lex clear --help`.

Every command also takes the display flags:

| Flag | Default | Description |
|---|---|---|
| `--color` | `auto` | `auto`, `always` or `never`; `auto` colours a terminal and honours `NO_COLOR` |
| `--symbols` | `letters` | Feedback tiles shown as `letters` (g/y/x), `digits` (2/1/0) or `emoji`; assist input accepts all three regardless |

## Languages

`english`, `french`, `german`, `spanish`, `italian`, `russian`

## Verbosity

```bash
RUST_LOG=info  cargo run --release -- ...   # per-game progress
RUST_LOG=debug cargo run --release -- ...   # each guess
RUST_LOG=trace cargo run --release -- ...   # entropy scores per candidate
```

## Development

```bash
pre-commit install    # install fmt + clippy hooks (once)

cargo test --all
cargo clippy --all --all-targets --all-features -- -D warnings
cargo fmt --all
```

## Workspace

| Crate | Description |
|---|---|
| `lex-data` | Corpus acquisition, Wiktionary validation, `Word`/`WordSet` types |
| `lex-core` | Entropy solver library: `Guesser`, `WordCorrectness`, game loop |
| `lex-cli` | Simulation CLI; monomorphizes `lex-core` over word lengths 3–21 |
