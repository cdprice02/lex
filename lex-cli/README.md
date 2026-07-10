# lex-cli

CLI for the `lex` Wordle solver: batch simulation against a language corpus, and an interactive assistant for live games.

## Usage

```bash
cargo run --release -- <COMMAND> [OPTIONS]
```

| Command | Description |
|---|---|
| `simulate` | Run batch games and report average guess count |
| `assist` | Interactively assist a live Wordle game |

## Shared flags

| Flag | Default | Description |
|---|---|---|
| `-l` / `--word-length` | `5` | Word length (3–10) |
| `--dictionary-length` | all | Cap candidate dictionary to N most-frequent words |
| `--lang` | `english` | Language: `english`, `french`, `german`, `spanish`, `italian`, `russian` |
| `--data-dir` | `data` | Root directory for cached word-frequency files |

`simulate` adds `-n` / `--num-games` (default: all words, cycling if > dictionary size).
`assist` adds `-s` / `--suggestions` (default: 5) — suggestions shown per turn.

## Assist mode

Each turn prints the top suggestions with entropy scores, then prompts for the
word you actually played (empty input accepts the top suggestion — useful when
Wordle rejects a word it doesn't know) and the feedback Wordle returned.

Feedback formats are auto-detected, one per entry (no mixing):

| Format | Correct | Misplaced | Absent |
|---|---|---|---|
| Letters (primary) | `g` | `y` | `x`, `b`, `.`, `-` |
| Digits | `2` | `1` | `0` |
| Emoji | 🟩 (🟧 high-contrast) | 🟨 (🟦 high-contrast) | ⬜ (⬛ dark mode) |

## Examples

```bash
# 100 five-letter English games
cargo run --release -- simulate -l 5 --lang english -n 100

# All Italian four-letter words with per-game logging
RUST_LOG=info cargo run --release -- simulate -l 4 --lang italian

# Top-2000 German words, 50 games, show each guess
RUST_LOG=debug cargo run --release -- simulate -l 5 --lang german --dictionary-length 2000 -n 50

# Assist today's Wordle, showing 3 suggestions per turn
cargo run --release -- assist -l 5 --suggestions 3
```

`RUST_LOG=trace` additionally prints entropy scores for every candidate on every turn.
