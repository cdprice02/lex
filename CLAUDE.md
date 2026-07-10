# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`lex` is a Wordle solver CLI that runs simulations against the Google Books Ngrams V3 corpus. It picks guesses by maximizing Shannon entropy over the remaining candidate word set. Words are filtered against Wiktionary extracts so every candidate is a real dictionary word. It is a generic solver: any supported (language, word length) pair, with no English-only or 5-letter-only assumptions.

The ordered implementation path, standing decisions, and per-phase task briefs live in `ROADMAP.md` and `.tasks/`.

## Commands

All commands require the nightly toolchain. Either `direnv allow` (Nix + flakes) or `rustup install nightly && rustup override set nightly` (rustup).

```bash
# Build / run
cargo build --release
cargo run --release -- -l 5 --lang english -n 100   # simulate 100 five-letter games

# Test (all crates)
cargo test --all

# Single test
cargo test --package lex-core suggest_filters_impossible_words
cargo test --package lex-data sums_across_years

# Benchmarks (nightly required, already pinned)
cargo bench --all

# Lint / format (mirrors pre-commit hooks)
cargo clippy --all --all-targets --all-features -- -D warnings
cargo fmt --all
```

The `RUST_LOG` env var controls verbosity (`info` shows per-game progress, `debug` shows each guess, `trace` shows entropy scores per candidate).

## Toolchain

Pinned to nightly in `rust-toolchain.toml`. The codebase uses several nightly features: `optimize_attribute`, `variant_count`, `non_exhaustive_omitted_patterns_lint`, and `extern crate test` for `#[bench]`. `.cargo/config.toml` enables parallel codegen (`-Z threads=8`, `-Z share-generics=yes`).

Toolchain options:
- **Nix + direnv**: `direnv allow` — picks up the pinned toolchain from `flake.nix` automatically
- **rustup**: `rustup install nightly && rustup override set nightly` — `rust-toolchain.toml` is read automatically

## Workspace structure

```
lex-data/     — corpus acquisition, Wiktionary validation, word types (crate: lex-data)
lex-core/     — entropy-based solver library and types (crate: lex_core)
lex-cli/      — simulation CLI, binary: lex (crate: lex_cli)
data/         — on-disk cache (created at runtime, not committed)
  ngrams/     —   binary ngrams caches: data/ngrams/{lang_code}/{N}.bin
  dicts/      —   Wiktionary word lists: data/dicts/{lang_code}.txt + .meta.json
```

Each crate has its own `CLAUDE.md` with internal architecture details.

## Pre-commit hooks

Install once with `pre-commit install`. Hooks run `cargo fmt --all -- --check` and `cargo clippy --all --all-targets --all-features -- -D warnings` before every commit.
