# CLAUDE.md: lex-cli

The `lex` frontend: a library (`lex_cli`, internal, no stable API) and the `lex` binary, whose `main.rs` parses, builds one rayon pool and dispatches. Subcommands: `simulate`, `assist`, `word`, `compare`, the cache groups `dict`, `corpus`, `profile`, `book`, and hidden `verdict`. Everything solver-side comes from `lex-core`, everything on disk from `lex-data`. Per-function detail lives in rustdoc.

## Module map

| Module | Owns |
|---|---|
| `cli/mod.rs` | `Cli` (global flags), `Command`, `build_pool`, `languages`; re-exports every argument type |
| `cli/play.rs` | `CommonArgs` (game and Strategy flags), `SimulateArgs`, `AssistArgs`, `CompareArgs`, the value parsers |
| `cli/book.rs`, `cli/cache.rs` | argument types of `book`; of `dict`, `corpus`, `profile`, plus `DataDirArg` |
| `config.rs` | `RunConfig { game, strategy, targets }`: the one value for what a run plays, built from flags or a Track |
| `tracks.rs` | `TRACKS`, the `const` Track table; grows with languages and lengths, no edit needed |
| `simulate.rs`, `lists.rs`, `targets.rs` | the simulate loop; `GameLists<N>` and `Source`; target selection |
| `books.rs`, `book_listing.rs` | Opening book key, load and build; `book list` and `book show` |
| `report.rs`, `compare.rs`, `verdict.rs` | the JSON `Report`; paired comparison of two reports; Track-snapshot verdicts |
| `assist.rs`, `grading.rs` | the interactive loop; the Luck and Skill wording as a pure function |
| `dicts.rs`, `corpora.rs`, `profiles.rs` | `list` and `show` of each cache; never download or build |
| `fetch.rs`, `clear.rs` | `dict fetch`, `corpus build`, `profile fetch` over a `Network` seam; every `clear` |
| `word/` | `lex word`: `lookup` reads caches into `Facts`, `render` prints them |
| `table.rs` | table or tab-separated output shared by every listing |
| `style.rs`, `progress.rs`, `error.rs` | colour and tiles; progress channel and `Observer`; `CliError` |

Integration tests: `tests/tracks.rs` (libtest-mimic, one ignored trial per Track, insta snapshots in `benchmarks/snapshots/`, data from `LEX_DATA_DIR`), `tests/explain_agrees.rs` (ignored), `tests/common/` (`parse`, `touch`); fixtures come from `lex_data::testing`.

## Invariants and gotchas

- **Dispatch.** `lex_data::match_word_length!(f, len, args)` calls `f::<N>` for N in 3..=21; `main.rs` uses it once per command. Keep `N` out of anything that does not touch a `Word<N>` (take the length as a value, convert at the end), since each generic is compiled 19 times.
- **Help text is an interface.** Doc comments on clap structs become `--help`; a move or rename must leave every subcommand's `--help` byte-identical. `Cli` has `about = None` so its doc does not become the banner.
- **Flags.** `--lang` and `-l` have no defaults: required unless `--profile`, which conflicts with both and with `--dictionary-length`. `--track` (hidden) conflicts with every game, strategy and target flag. `--targets` conflicts with `-n`; `--seed` needs `-n`. `--force` conflicts with `--offline`.
- **Skipped fields.** `main` copies global `--symbols`, `--download-threads` (via `Cli::download_request`, which treats an explicit `--threads` as the request) and `--offline` into `#[arg(skip)]` fields of the args types; `data()` and `dir()` build the `DataDir` from them. None reaches the book key, Report or snapshots. `--offline` forbids network requests only: computed work still runs.
- **Plain output is an interface.** Piped output of `list`, `show`, `word` and `compare` is tab-separated with one header line; `benchmarks/scripts/prior-screen.sh` greps `compare`'s plain lines. The book footer goes to stderr when piped.
- **Never fetch.** `dict`, `corpus`, `profile` `list`/`show`, `book list`/`show` and `word` read disk only; an uncached item is an error naming the fetch command.
- **Fetch.** `Network` is the download seam (tests stub it); `fetch.rs` calls `DataDir::require_online` before each call. Cascade upward: `corpus build` fetches an absent Dictionary, `profile fetch` builds an absent Corpus. Cached means ready; `--force` runs the matching `clear` first.
- **Books.** A book always covers the full Answer set, whatever `-n` samples. `books::guesser` keeps scores in memory only when asked (`assist` without `--no-explain`). Build time goes to the `Observer`, never into the Report.
- **Progress.** One channel, `Mode` of `Bar`, `Lines` or `Off`; library callers and tests pass `None` and draw nothing. `init` runs once in `main`.
- **Sharing the decision tree** across games is valid only while `Candidates::suggest` is deterministic (`lex-core/CLAUDE.md`).
- **Report prior.** `simulate` weights results with `answer_prior` at `CANONICAL_TEMPERATURE` over the whole pool, whatever `--temperature` the solver used; only targets are looked up.
- **Track tests** play each Track in its own rayon pool, 1 thread unless `LEX_TRACK_THREADS` says otherwise (a snapshot must not move with it); a behavior-changing PR commits snapshots in a separate `test(snapshots)` commit (ADR 0012). `verdict` takes the digest as a parameter and reads a Track's tier from `TRACKS`, never the snapshot.
- **Assist.** Feedback format is auto-detected from the first symbol (letters `g y x` with aliases `b . -`, digits `2 1 0`, emoji with dark and high-contrast variants) and must not mix within an entry; `--symbols` only changes display. With `--profile`, a played word outside the Guess pool is rejected. The winning turn grades Skill only; a played word outside the Guess pool gets no grading line. Realised bits are built only under `--details`.
- **Reports.** Schema v1; older reports read with defaults (no `strategy` reads as ultrahard, T=5, win weight 0; `hard_mode` reads as ultrahard or unconstrained). `compare` hard-errors when the reports played different Profiles or only one did. Failure means more than 6 guesses.
- **Errors.** Argument validation uses `CliError`; runtime errors from `lex-data` and `lex-core` stay `anyhow` (typed errors are #163).
