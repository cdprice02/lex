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
book, below) and the cache groups `dict`, `corpus`, `profile` and `book`, each
with a `clear` command (below).

`simulate` and `assist` build the opening book for their game and strategy on
first use (progress on stderr) and load it afterwards; `--no-book` plays fully
online. The book is a cache of the solver's own choices, so games are identical
either way. `lex book build` takes the same game and strategy flags, builds the
book or reports it up to date (`--force` rebuilds), and prints its size and
build time. Books live in `data/books/`; `lex book clear --lang L [-l N]`
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

## Cache commands

Ready the caches ahead of time, like `cargo fetch`, so later runs need no network:

| Command | Does |
|---|---|
| `lex dict fetch --lang L` (or `--all`) | downloads the Dictionary of `L` |
| `lex corpus build --lang L` (or `--all`) | builds the Corpus of `L` at every length (fetching its Dictionary first if absent); `-l` is not accepted |
| `lex profile fetch --profile P` (or `--all`) | fetches and verifies the lists of `P` (building its language's Corpus first if absent) |

Each is a no-op when the item is cached (it says it is ready, and when it was fetched) and redoes the work with `--force`: `dict fetch --force` warns that it takes KAIKKI's current (rolling) extract and first clears what was built from the old one (its Corpus caches and their books); `corpus build --force` clears the Corpus and its books and rebuilds from the cached Dictionary; `profile fetch --force` clears the lists and their books and re-verifies the checksums. Downloads show a spinner or shard bar on stderr (`--no-progress` silences it), and the command ends with the matching `show` output (for `corpus build`, the `corpus list` rows of the language). Opening books stay with `lex book build`.

Each kind of cached data is a group with a `clear` command that deletes it so it is rebuilt on next use. Every `clear` prints each path it removed and a total, and asks nothing first. Naming nothing is an error; `--all` clears every item of that kind.

| Command | Removes |
|---|---|
| `lex dict clear --lang L` (alias `dictionary`) | the Dictionary of `L` (its Wiktionary word list and metadata), its Corpus caches at every length and their opening books; `-l` is not accepted |
| `lex corpus clear --lang L [-l N]` | the Corpus cache of `L` (one length with `-l`) and its opening books |
| `lex profile clear --profile P` | the fetched lists of `P` and its opening books |
| `lex book clear --lang L [-l N]` | only the opening books |
| `lex book clear --stale [--lang L] [-l N] [--profile P]` | only the opening books `lex book list --stale` shows: stale or corrupt |

A `clear` of something that is not cached succeeds with "0 paths removed".

`lex book list [--lang L] [-l N] [--profile P] [--stale]` shows a row per stored opening book (8-character id, game, the Strategy fields that differ from the default, positions, size, status) and a footer of counts by status and total size. A book is **current**, **stale** (a run of its game and Strategy would not read it: its solver digest, Corpus checksum or Profile list checksums are not the current ones; the fields are named) or **corrupt** (unreadable header or key, or a body that fails its checksum). A different Strategy is a different book, never a stale one. `lex book show ID` takes an id (or a unique prefix) from `list`, or the game and Strategy flags of `book build`: it prints every key field, the header, whether the body checksum verifies, the path and size, and the stored and current value of each stale field; for flags it shows the book that run would read, or the nearest stored book and the fields that differ. Neither command builds a book or downloads anything. Piped, the output is tab-separated with one header line and sizes in bytes, and `book list`'s footer of counts goes to stderr, so stdout is only the header and one line per book.

`lex dict list` shows a row per cached Dictionary (language, words, fetch date, checksum prefix, size, Corpus lengths built against it) and `lex dict show --lang L` shows one in full: Provenance, paths, word counts per word length 3 to 21 and which lengths have a Corpus cached. Both read the cache only and never download; a language that has not been used yet is absent. On a terminal they print tables; piped, tab-separated columns with one header line and sizes in bytes.

`lex word <WORD>... --lang X` looks words up in the cache and says where each stands on the path to being a Candidate: the shape (one alphabetic token after NFC and lowercasing), a length in 3 to 21 that the language's Corpus supports, Dictionary membership, Corpus rank of the Corpus size with its frequency and Prior, and, for each Profile at that language and length, Answer list and Guess pool membership. The verdict names the first step that fails, or reads "a Candidate in Corpus runs (rank R of M)". The Prior is the probability of being the answer at game start under the default Strategy (labelled so), over the Corpus, and over the Answer list for a Profile. Each row has the normalized `word` and an `input` column holding what was typed (empty when it is already normalized). On a terminal one word prints the step-by-step report; several words, or a pipe, print a row per word as a table or as tab-separated columns with one header line. It reads the cache only and never fetches or builds: a Dictionary, Corpus or Profile that is not cached reads "not cached" (or "not fetched") and the later steps read unknown. It exits 0 whether or not a word is a Candidate, and 1 only when a cached file cannot be read.

All take `--data-dir`; the group commands accept `--all` in place of the selector. `dict clear` warns first: the next fetch takes KAIKKI's current (rolling) Wiktionary extract, which can change the Corpus and that language's Track snapshots. See `lex <group> clear --help`.

Every command also takes the display flags:

| Flag | Default | Description |
|---|---|---|
| `--color` | `auto` | `auto`, `always` or `never`; `auto` colours a terminal and honours `NO_COLOR` |
| `--symbols` | `letters` | Feedback tiles shown as `letters` (g/y/x), `digits` (2/1/0) or `emoji`; assist input accepts all three regardless |
| `--no-progress` | off | Show no progress: bars and spinners on a terminal, periodic lines elsewhere, and the opening-book build messages. The summary, errors and `RUST_LOG` logging stay |
| `--threads` | available parallelism (1 if unknown) | Worker threads, at least 1. `main` builds one rayon pool of this size and runs the command inside it |
| `--download-threads` | 4 (a placeholder until measured), or `--threads` when given | Parallel shard downloads, at least 1, capped at the language's shard count. Wins over `--threads`; only a `--threads` given on the command line counts |

## Languages

`english`, `french`, `german`, `spanish`, `italian`, `russian`

## Progress

On a terminal, `simulate` draws a bar over the games on stderr (count, elapsed time, games per second, ETA), and building an opening book shows a spinner with the positions booked. Off a terminal, a line goes to stderr every 5 seconds instead. `--no-progress` turns both off. With `RUST_LOG` unset the log filter is `error,lex::progress=info`; setting `RUST_LOG` replaces it.

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
