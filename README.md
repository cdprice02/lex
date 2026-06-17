# lex

An entropy-based Wordle solver. Simulates games against the [Google Books Ngrams V3](https://storage.googleapis.com/books/ngrams/books/datasetsv3.html) corpus, cross-referenced with [Wiktionary](https://kaikki.org) to keep only real dictionary words. Picks each guess by maximizing Shannon entropy over the remaining candidate set.

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
cargo run --release -- -l 5 --lang english -n 100
```

The first run for a language downloads its Wiktionary word list and all Google Books Ngrams shards, then caches everything to `data/`. Subsequent runs are fully offline.

## CLI flags

| Flag | Default | Description |
|---|---|---|
| `-l` / `--word-length` | `5` | Word length (3–10) |
| `--dictionary-length` | all | Cap the candidate dictionary size |
| `-n` / `--num-games` | all | Number of games to simulate |
| `--lang` | `english` | Language corpus (see below) |
| `--data-dir` | `data` | Directory for cached word-frequency files |

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
| `lex-cli` | Simulation CLI; monomorphizes `lex-core` over word lengths 3–10 |
