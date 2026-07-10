# Roadmap

Ordered implementation path. Each phase has a task brief in `.tasks/` with
full context, requirements, and open questions — briefs are written to be
handed to another agent for detailed planning and execution (executor
conventions in `.tasks/README.md`).

Scope constraint: **lex is a generic solver** for any supported
(language, word length) pair. No English-only or 5-letter-only assumptions in
storage formats, strategy code, or benchmarks.

## Phases

| # | Task | One-liner |
|---|------|-----------|
| [00](.tasks/00-ci.md) | CI ✅ | GitHub Actions on the pinned nightly: fmt, clippy, tests |
| [01](.tasks/01-assist-mode.md) | Assist mode ✅ | Interactive helper + subcommand split; dogfood the daily Wordle |
| [02](.tasks/02-measurement.md) | Measurement & provenance | The yardstick: seeded targets, JSON output, histogram + failure rate, corpus stamping, baselines, golden test |
| [03](.tasks/03-data-integrity.md) | Data integrity | Loud load-time validation; versioned cache header |
| [04](.tasks/04-memoization.md) | Decision-tree memoization | Simulation as one pattern-keyed tree; full-dictionary eval becomes cheap |
| [05](.tasks/05-strategy.md) | Strategy generalization | Unconstrained guessing; strategy abstraction; prior moves to lex-core |
| [06](.tasks/06-micro-opt.md) | Micro-optimization | Selective resets; `correct()` alternatives; alphabet-indexed words |
| [07](.tasks/07-pipeline-sync.md) | Pipeline simplification | Drop tokio for sync HTTP; retry; stream-time filtering |
| [08](.tasks/08-endgame.md) | Endgame exact search | Expectimax when few candidates remain |
| [09](.tasks/09-simd.md) | SIMD | `portable_simd` over the phase-06 representation |
| [10](.tasks/10-multithreading.md) | Multithreading | Parallel games/candidates |

Ordering rationale:

- **CI first** — the upstream exists; every later phase gets a green/red
  signal for free.
- **Assist (01) before measurement (02)** — it changes no solver behavior, so
  it doesn't need the measurement gate, and it pays twice: daily-Wordle
  dogfooding from the start, and a manual test harness for the rest of the
  roadmap. Strategy ideas discovered while dogfooding still wait for 02's
  baselines before landing.
- **02 gates all solver-behavior changes (04, 05, 08)** — nothing lands
  without a paired per-target before/after against committed baselines.
- **03 before 04** — memoization and any persisted artifact assume validated
  data and recorded corpus identity.
- **07 is order-flexible** — cold-path cleanup; slot it wherever convenient
  after 00.
- **09–10 last** per the optimization order: single-threaded first, then
  SIMD, then threads.

## Unscheduled

- `#![warn(missing_docs)]` per crate — anytime; recommended early, since doc
  comments are context for the agents executing later tasks.
- `lex-tui` crate (ratatui) — after 01 proves the interaction loop; separate
  crate per the note in `lex-cli/src/simulate.rs`.
- `dict` CLI subcommand (list/meta/clear) — whenever useful for debugging.

## Decisions

- **Nightly toolchain — deliberate.** Chosen for optimization headroom and
  `#[bench]` experimentation; phase 09 (`portable_simd`) makes it
  load-bearing. Pinned in `rust-toolchain.toml`, reproducible via the flake.
  Exit path if ever needed: `variant_count` → hardcoded const, `#[bench]` →
  criterion/divan, `optimize_attribute` → drop.
- **No git-lfs; no committed corpus data.** Everything is reproducible from
  public sources; integrity comes from header checksums + provenance stamps.
- **Generic answer model.** The frequency-derived prior is canonical for
  every (lang, N); curated answer lists (e.g. NYT English-5) are optional
  evaluation overlays only.
- **Crate structure stays** (`lex-data` / `lex-core` / `lex-cli`).
- **`reqwest::blocking` rejected** for the phase-07 sync rewrite — it still
  embeds a tokio runtime; ureq drops the async stack entirely.
- **SIMD and multithreading deferred** to phases 09–10: single-threaded
  optimization completes first.

## Nightly feature candidates (audited 2026-07)

In use: `optimize_attribute`, `variant_count`,
`non_exhaustive_omitted_patterns_lint`, `must_not_suspend`, `test`
(`#[bench]`), `-Z threads`, `-Z share-generics`, `profile-rustflags`.

Worth adopting:

- `portable_simd` — phase 09; the feature that makes nightly pay for itself
- `-Z build-std` with the existing `target-cpu=native` — recompile std with
  native codegen + LTO; build-config-only experiment, benchmark before
  keeping
- PGO (stable; listed for synergy) — the deterministic simulator is an ideal
  profiling workload; combine with build-std

Experiment-grade (adopt narrowly or wait):

- `generic_const_exprs` — stack `[f64; 3^N]` pattern buffers instead of the
  thread-local Vec; the feature is still incomplete and ICE-prone
- `TrustedLen` for `WordIter` once phase 03 makes it exact-size
- `iter_array_chunks` for record parsing (stable `slice::as_chunks` may
  already suffice — check first)

Note: `#[optimize(speed)]` is currently decorative — it only counteracts
size-optimization, and the release profile is already `opt-level = 3`. Kept
as insurance for other profiles; drop if it causes friction (revisit in
phase 06).
