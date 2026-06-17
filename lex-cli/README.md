# lex-cli

Simulation CLI for the `lex` Wordle solver. Runs batch games against a language corpus and reports average guess count.

## Usage

```bash
cargo run --release -- [OPTIONS]
```

## Flags

| Flag | Default | Description |
|---|---|---|
| `-l` / `--word-length` | `5` | Word length (3–10) |
| `--dictionary-length` | all | Cap candidate dictionary to N most-frequent words |
| `-n` / `--num-games` | all | Number of games to simulate (cycles through word list if > dictionary size) |
| `--lang` | `english` | Language: `english`, `french`, `german`, `spanish`, `italian`, `russian` |
| `--data-dir` | `data` | Root directory for cached word-frequency files |

## Examples

```bash
# 100 five-letter English games
cargo run --release -- -l 5 --lang english -n 100

# All Italian four-letter words with per-game logging
RUST_LOG=info cargo run --release -- -l 4 --lang italian

# Top-2000 German words, 50 games, show each guess
RUST_LOG=debug cargo run --release -- -l 5 --lang german --dictionary-length 2000 -n 50
```

`RUST_LOG=trace` additionally prints entropy scores for every candidate on every turn.
