# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`lex` is a Wordle solver CLI that runs simulations against the Google Books Ngrams V3 corpus. It picks guesses by maximizing Shannon entropy over the remaining candidate word set, scored against every word in the dictionary by default (`--pool hard` plays the official Wordle hard-mode rule; `--pool ultrahard` restricts guesses to words consistent with every feedback, which on a Corpus run are the candidates). Words are filtered against Wiktionary extracts so every candidate is a real dictionary word. It is a generic solver: any supported (language, word length) pair, with no English-only or 5-letter-only assumptions.

Planned work lives in GitHub issues; the open `wayfinder:map` issue charts the current effort. Standing decisions are ADRs in `docs/adr/`. A solver-behavior change ships with a paired per-target comparison judged by the rule in `docs/adr/0007-benchmark-objective.md`; every Track has a committed snapshot in `benchmarks/snapshots/` (ADR 0012), checked with `cargo test --release -p lex-cli --test tracks -- --ignored headline`, and a behavior-changing PR commits its snapshots in a separate `test(snapshots): ...` commit. Methodology and current results are in `benchmarks/README.md`.

## Commands

All commands require the nightly toolchain. Either `direnv allow` (Nix + flakes) or `rustup install nightly && rustup override set nightly` (rustup).

```bash
# Build / run
cargo build --release
cargo run --release -- simulate -l 5 --lang english -n 100   # simulate 100 five-letter games
cargo run --release -- assist -l 5 --lang english             # interactively assist a live game

# Test (all crates)
cargo test --all

# Single test
cargo test --package lex-core suggest_filters_impossible_words
cargo test --package lex-data sums_across_years

# Micro-benchmarks (divan; the bench feature gates them)
cargo bench -p lex-core --features bench
cargo bench -p lex-data --features bench
cargo bench -p lex-core --features bench -- ordinal_count   # divan filter: a substring of the bench path
cargo bench -p lex-core --features bench -- --exact correctness::benches::ordinal_count   # exact path
cargo test --workspace --all-features --benches             # divan test mode: each bench runs once

# Lint / format (mirrors pre-commit hooks)
cargo clippy --all --all-targets --all-features -- -D warnings
cargo fmt --all
```

The `RUST_LOG` env var controls verbosity (`info` shows per-game progress, `debug` shows each guess, `trace` shows entropy scores per candidate).

## Toolchain

Pinned to nightly in `rust-toolchain.toml`. The codebase uses nightly features such as `non_exhaustive_omitted_patterns_lint` and `must_not_suspend`; benches use divan, not `#[bench]`. `.cargo/config.toml` enables parallel codegen (`-Z threads=8`, `-Z share-generics=yes`).

Toolchain options:
- **Nix + direnv**: `direnv allow` — picks up the pinned toolchain from `flake.nix` automatically
- **rustup**: `rustup install nightly && rustup override set nightly` — `rust-toolchain.toml` is read automatically; also `cargo install cargo-insta` for the Track snapshots (the Nix shell has it)

## Workspace structure

```
lex-data/     — corpus acquisition, Wiktionary validation, word types (crate: lex-data)
lex-core/     — entropy-based solver library and types (crate: lex_core)
lex-cli/      — simulation CLI, binary: lex (crate: lex_cli)
data/         — on-disk cache (created at runtime, not committed)
  ngrams/     —   binary ngrams caches: data/ngrams/{lang_code}/{N}.bin
  dicts/      —   Wiktionary word lists: data/dicts/{lang_code}.txt + .meta.json
  books/      :   opening books: data/books/{lang_code}/{N}/{key-hash}.bin, built on first use by simulate and assist (`lex book build`)
```

Each crate has its own `CLAUDE.md` with internal architecture details.

## Lints

`[workspace.lints]` in the root `Cargo.toml` flags undocumented items (`missing_docs`, clippy `missing_docs_in_private_items`); CI fails on them through clippy `-D warnings`. Test and bench code carries `expect`s instead.

## Pre-commit hooks

Install once with `pre-commit install`. Hooks run `cargo fmt --all -- --check` and `cargo clippy --all --all-targets --all-features -- -D warnings` before every commit.
