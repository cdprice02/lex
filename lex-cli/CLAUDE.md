# CLAUDE.md — lex-cli

## Purpose

CLI frontend, a library (`lex_cli`, internal: `src/lib.rs`) plus the `lex` binary (`main.rs` parses and dispatches), with these subcommands (`verdict` is hidden plumbing): `simulate` (batch games via `lex-core`),
`assist` (interactive helper for live games), `word` (look words up), `compare` (paired diff of two
simulation reports), and four cache groups (see Cache commands): `dict`, `corpus`, `profile` and `book` (`book build` builds or refreshes the opening book). Loads corpora via `lex-data` and
monomorphizes the call chain over word lengths 3–21 at compile time.

## Module map

| Module | Responsibility |
|---|---|
| `cli.rs` | clap `Cli`/`Command`; `CommonArgs` flattened into `SimulateArgs`/`AssistArgs`; `--lang` and `-l` have no defaults: each is required unless `--profile` is given and conflicts with it (the Profile fixes both); `--profile` also conflicts with `--dictionary-length`; `--pool <all|hard|ultrahard>` (parsed by `GuessPool`'s `FromStr`) picks the Guess pool; `CommonArgs::dispatch_length()` picks the length to monomorphize over, `CommonArgs::config(targets)` builds the `RunConfig`; `SimulateArgs::config()` turns `-n`/`--seed`/`--targets` into `Targets` (`--targets` conflicts with `-n`; `--seed` needs `-n`), and the hidden `--track <id>` plays a Track's config and conflicts with every game, pool, strategy and target flag |
| `config.rs` | `RunConfig { game, strategy, targets }`: `Game` (`Corpus { language, length, dictionary_length }` or `Profile`) and `Targets` (`Full`, `Sample { n, seed }`, `File`). One value for what a run plays, built from flags or from a Track |
| `tracks.rs` | `TRACKS`, the `const` table of benchmark Tracks (`Track { game, strategy, tier, judged, targets }`), built by a `const fn` over `Language::VARIANTS` and the length range; `TRACK_COUNT` is computed from them, so a new language or length needs no edit. `Track::config()`, `Track::find(id)`, ids from `Display`. Tier and target rules are ADR 0007's; the evidence is `benchmarks/experiments/track-targets.md` |
| `books.rs` | Opening book files for a run: `key()` builds the `BookKey` (Corpus and Profile checksums, length, pool size, `Strategy::key_fields()`, `SOLVER_DIGEST`); `ensure()` loads the book, or on a miss builds it (reporting to an `Observer`; a stale book's start message names the changed fields) and stores it, errors on a damaged file naming it and `lex book build --force`; `guesser()` returns a `Guesser` seeded with it unless `BookMode::Off` (`--no-book`); `build_command` is `lex book build`. A book always covers the full Answer set, whatever `-n` samples. Build time goes to the observer and `book build`'s stdout, never into the Report |
| `clear.rs` | The `clear` verb of the cache groups: `dict`, `corpus`, `profile`, `book` each call the matching `DataDir` delete (which returns the paths it removed) and print them through `summary()`, one per line and a total. `dict` warns on stderr first |
| `dicts.rs` | `lex dict list` and `lex dict show --lang L`: `render_list` and `render_show` take the `DictRecord`s of `DataDir::dicts()` / `dict()` and a `terminal` flag (`is_terminal()` of stdout): a rounded `tabled` table with size in MiB (one decimal) on a terminal, otherwise tab-separated columns with one header line and size in bytes (`show` is `field`, `value`, `note` rows). `list` has a row per cached language (code, words, fetch date, 8-character checksum, size, Corpus lengths cached) or prints "no Dictionaries cached"; `show` prints full Provenance, paths, per-length counts 3 to 21 with cached Corpus lengths noted, the count outside, and flags a metadata word count that differs from the file; an uncached language is an error. Provenance missing reads "unknown". Neither command downloads |
| `book_listing.rs` | `lex book list` and `lex book show`, rendered like `dicts.rs` (`render_list`, `render_show`, `footer`; table with KiB/MiB sizes on a terminal, tab-separated with bytes piped; the footer prints under the table on a terminal and to stderr when piped, so stdout stays a header and one line per book). `list` rows: 8-character id, game (`english-5` or the Profile's name), Strategy fields differing from `Strategy::DEFAULT` as `name=value`, positions, size, status (`current`, `stale (fields)`, `corrupt`); `--stale` keeps what `book clear --stale` would remove. `show ID` takes an id or unique prefix; with game and Strategy flags (as `book build`) it keys the run through `books::key` and shows that book, else the nearest sibling (`DataDir::nearest_book`) and the differing fields, and errors rather than fetching when the Corpus or Profile lists are not cached. `show` verifies the body checksum; `list` verifies every body checksum too, so a damaged body lists as `corrupt`. `book clear --stale` (`clear.rs`) passes `SOLVER_DIGEST` to `DataDir::clear_stale_books` |
| `corpora.rs` | `lex corpus list [--lang L] [-l N]` and `lex corpus show --lang L -l N`, in `dicts.rs`'s output style (its `render_rows`, `row`, `flag`, `size` are shared). `list`: language, length, words, supported, size, 8-character checksum, status from headers only; `show` adds path, format version, the full checksum verified against the records, the ngrams snapshot (`20200217 (this build)`), the Dictionary fetch time, and a warning with a rebuild hint when the Corpus is older than its Dictionary. "no Corpora cached" when empty; an uncached Corpus is an error. Never builds |
| `profiles.rs` | `lex profile list` (profile, language, length, Answer and guess-only counts, fetch date, verified against the compiled-in checksums, Corpus cached) and `lex profile show --profile P` (per list: path, source URLs, expected and actual checksum, word count). "no Profiles fetched" when empty; an unfetched Profile is an error. Never fetches |
| `word.rs` | `lex word <WORD>... --lang L [--data-dir D]`, read-only: `look_up` normalizes each input as the build does (`lex_data::normalize`, `is_valid_word`; NFC and lowercase; the typed form is kept as `input`) and walks the path in order: shape, length in 3..=21, Dictionary membership (`DataDir::dict_members`, one pass over the word list), Corpus (`DataDir::cached_corpus`; under `MIN_SUPPORTED_WORDS` is Unsupported), rank, frequency and the Prior, then each Profile at that language and length (`DataDir::profile_words`: Answer list, guess-only, or not fetched). The Prior is `lex_core::answer_prior` at `Strategy::DEFAULT.temperature` (the default Prior shape is `Power`, which a test pins), over the Corpus, and over the Answer list for a Profile answer (Corpus frequencies, the Corpus's least frequency for an answer it lacks); labelled "(default Strategy)". A cache that is not there reads "not cached" with `dict show`'s and `corpus show`'s hint (`dicts::not_cached_hint`, `corpora::not_cached_hint`) and later steps unknown; nothing is fetched or created. `verdict()` names the first failing step or "a Candidate in Corpus runs (rank R of M)". Output: a row per word (word, input, length, dictionary, rank, frequency, prior, profiles, verdict) as a table or tab-separated lines with one header; `word` holds only the normalized form and `input` what was typed, empty when equal; a single word on a terminal gets the step-by-step `step`/`result`/`note` report, with an `input` row when it differs. Words are grouped by length and dispatched with `match_word_length!` |
| `simulate.rs` | `run(&RunConfig, &DataDir, Option<&dyn Observer>) -> Result<Report>` plays a config with its book, built on first use (`run_with(.., BookMode, observer)` opts out), and prints nothing (it dispatches the word length itself; the report assembly is not generic); `simulate::<N>(&SimulateArgs)` is the command: `args.config()`, `run`, then print or write the report |
| `lists.rs` | `GameLists<N>` (pool, Answer list; a Corpus run answers with every pool word) and `Source` (`Corpus` or `Profile`, with the report's Corpus info and `ProfileStats`), both from `Source::load`. `Source` owns `dictionary_length()` (the Corpus size, fixed at load), `answer_noun()`, the report's provenance and assist's Guess-pool check; a new source is a new variant |
| `targets.rs` | target selection: seeded uniform sample (ChaCha8, partial Fisher-Yates) or `--targets` file |
| `tests/tracks.rs` | libtest-mimic harness (`harness = false`): one ignored trial per `TRACKS` entry, named `<tier>::<track-id>`, that plays `simulate::run(&track.config(), &data_dir, None)` (which builds a missing Opening book first, like `simulate`) and compares an insta snapshot in `benchmarks/snapshots/` (body: Track id, config, Corpus and Profile checksums, summary, first guess, games SHA-256; header `info`: `SOLVER_DIGEST`, lex version). Data comes from `LEX_DATA_DIR` (default the workspace `data/`); missing data fails with a download hint. Loop: `cargo test --release -p lex-cli --test tracks -- --ignored headline` (fast set), `-- --ignored` (full set), `cargo insta test --force-update-snapshots` to refresh header digests after a behavior-preserving change; behavior-changing PRs commit snapshots in a separate `test(snapshots): ...` commit (ADR 0012) |
| `verdict.rs` | `lex verdict`, see Plumbing |
| `report.rs` | JSON schema (`Report`), uniform and prior-weighted `Stats`, text summary: `render()` is plain text (snapshot-tested), `render_styled(true)` adds bold means, dim provenance, green bars within `MAX_GUESSES` and red failure rows, printed through `anstream` |
| `compare.rs` | paired per-target comparison of two reports, joined on word. `comparison()` returns a `Comparison` printed two ways: `render_table()` (a `tabled` rounded table, coloured through `style`) when stdout is a terminal, `render_plain()` otherwise. The plain lines (`mean guesses (uniform): ...`) are an interface: `benchmarks/scripts/prior-screen.sh` greps them |
| `assist.rs` | interactive loop; `parse_feedback` (private) auto-detects input format whatever `--symbols` says; the echoed feedback is `style::tiles` |
| `style.rs` | shared by every frontend, knows no subcommand: `ColorArg` (`--color auto\|always\|never`, maps to `anstream::ColorChoice`), `SymbolScheme` (`--symbols letters\|digits\|emoji`, display only), `init(color)` sets anstream's global policy, `tile_style` is the palette, `tiles()` renders feedback (padded coloured cells for letters and digits, bare emoji). Print styled text through `anstream::println!`, which strips escapes off a terminal or under `NO_COLOR`. Both flags are global on `Cli`; `main` copies `--symbols` into `AssistArgs` |
| `progress.rs` | Progress for the long phases, one channel with three `Mode`s from `mode(stderr_is_terminal, no_progress)`: `Bar` (indicatif spinner and games bar on stderr, logger wrapped by `indicatif-log-bridge` so log lines print above), `Lines` (every 5 s, `log::info!` under target `lex::progress`), `Off`. `init(no_progress)` runs once in `main`, installs the logger (default filter `error,lex::progress=info`, replaced by `RUST_LOG`) and the mode; unset means `Off`. The `Observer` trait (`&self` methods, so each observer keeps its one piece of mutable state, the bar or the `Phase`, behind a `RefCell` or `Cell` it alone owns: `book_started`, `book_progress`, `book_finished`, `games_started`, `game_played`) is what `run`, `run_with`, `books::ensure` and `books::guesser` take as `Option<&dyn Observer>`; `progress::observer()` returns the active mode's. Library callers, `book_files.rs` and the Track tests pass `None` and draw nothing. `--no-progress` is global; it also silences the book start message ("no opening book for X; building it", with "the closest on disk differs in ..." when a stale book exists) and "built the opening book" |
| `error.rs` | `CliError` |

`--threads N` is a global flag on `Cli` beside `--color`, not a `RunConfig` field: a `NonZeroUsize` (clap rejects 0), default `default_threads()` (`available_parallelism()`, else 1), no environment variable. `main` builds one rayon pool with `cli::build_pool` (a failure is `CliError::ThreadPool`, the usual error exit) and runs the dispatched command inside `pool.install`, so lex-data reads the budget from `rayon::current_num_threads()`. The count never reaches the book key, the Report or the Track snapshots; `tests/tracks.rs` plays each Track in its own 1-thread pool so the solver stays serial per Track while Tracks run across libtest threads.

## Cache commands

Game actions stay top-level (`simulate`, `assist`, `compare`, `verdict`). Each kind of cached data is a group; `dict`, `corpus` and `profile` have `list` and `show` beside `clear`, and later slices add them to `book` (tracker #23).

| Group | `clear` | Cascade |
|---|---|---|
| `lex dict` (alias `dictionary`) | `--lang L` or `--all` | the Dictionary (`dicts/{code}.txt` and `.meta.json`), that language's Corpus caches at every length, their books; warns that the next fetch takes KAIKKI's rolling extract, which can change the Corpus and its Track snapshots; no `-l` (one Dictionary serves every length) |
| `lex corpus` | `--lang L [-l N]` or `--all` | the Corpus cache and its books |
| `lex profile` | `--profile P` or `--all` | the Profile's lists and its books |
| `lex book` | `--lang L [-l N]`, `--all`, or `--stale [--lang L] [-l N] [--profile P]` | books only (`--stale`: those `book list --stale` shows); `book build`, `list` and `show` also live here |

`--lang`, `-l`, `--profile` and `--data-dir` mean the same in every group. `clear` with nothing selecting what to clear is a clap error unless `--all`. The deletes live in `lex-data` (`DataDir::{clear_dict, clear, clear_profile, clear_books}`) and return the paths they removed; a `clear` of something that is not cached removes nothing and succeeds with "0 paths removed". There is no top-level clear command. `tests/cache_commands.rs` covers parsing and the cascades in temp data dirs.

## Plumbing

`lex verdict` is hidden from `--help`; CI and the benchmark loop use it, people rarely do.
`lex verdict <base-dir> <head-dir>` reads two directories of Track snapshots (the YAML header via `yaml_serde` for the solver digest, the body as JSON) and prints a table and an `Outcome:` line by ADR 0007's rule (a coloured `tabled` table on a terminal via `Verdict::render_table`; markdown via `Verdict::render` when piped, which CI appends to the job summary and `tests/verdict.rs` parses); exit 0 accept, unchanged, rebaseline or new, 1 reject or a stale headline digest, 2 bad input. A Track's tier and judged flag come from `TRACKS` by id, never the snapshot. `lex verdict --readme <snap-dir> [--check] [--file PATH]` rewrites, or checks, the headline table between `<!-- tracks:start -->` and `<!-- tracks:end -->` in `benchmarks/README.md`. `verdict()` takes the expected digest as a parameter; `tests/verdict.rs` runs the binary over `tests/fixtures/verdict/` (a `base/` set and per-case head overlays, with `FIXTURE_DIGEST` replaced by the compiled digest). Methodology: `benchmarks/README.md`.

## Monomorphization dispatch

`lex_data::match_word_length!(f, len, args)` expands (via `seq_macro`) to a
`match` on `len` calling `f::<N>(args)` for each `N` in 3..=21. The macro lives
in `lex-data` next to the `MIN_WORD_LENGTH`/`MAX_WORD_LENGTH` consts it mirrors
(compile-time assert keeps them in lockstep), so future frontends reuse it.
`main.rs` invokes it once per subcommand and returns an `ExitCode`; `verdict::command` does too.

## Simulate loop (`simulate.rs`)

1. Load `GameLists<N>` and its `Source` from the `RunConfig`; capture the evaluation prior over the whole pool (`answer_prior` at `CANONICAL_TEMPERATURE`, whatever the solver's `--temperature`); only targets are looked up. `corpus.dictionary_length` is `Source::dictionary_length()`, the Corpus size (`DataDir::ngrams_count` on a Profile, the pool size otherwise)
2. Select targets from the Answer list, or the whole dictionary without a Profile (`Targets`: all, a seeded sample, or a file); file words must be in that list
3. Build one `Guesser` from the config's `Strategy` (`--pool`, `--temperature`, `--win-weight`), seeded with the Opening book (`books::guesser`; `--no-book` skips it) and read `first_guess` from its root suggestion (reported, not replayed)
4. For each game: call `play(word, &mut guesser, Vec::new())`; games share the guesser's decision tree, so each position is scored once
5. Emit the report: summary to stdout (stderr when `-o -` sends JSON to stdout), JSON to `-o PATH`

Sharing the tree is only valid while `Candidates::suggest` is deterministic (see `lex-core/CLAUDE.md`).

## Assist loop (`assist.rs`)

With `--profile`, a played word outside the Profile's Guess pool is rejected ("is not in the `<profile>` guess pool"); without one any `Word<N>` is accepted.

Per turn: show `suggest_top_k(--suggestions)` with entropy bits (non-candidate probes are marked) → prompt for
the word actually played (empty input = top suggestion; may be any `Word<N>`,
not necessarily suggested or in the corpus) → prompt for feedback →
`push_guess`. Exits on all-correct feedback, candidate exhaustion (target not
in corpus / inconsistent entry), or EOF.

Feedback formats are auto-detected from the first symbol and must not be mixed
within one entry: letters `g`/`y`/`x` (gray aliases `b` `.` `-`), digits
`2`/`1`/`0` (matches the `Correctness` encoding), emoji including dark-mode ⬛
and high-contrast 🟧/🟦 variants.

## Error handling

CLI argument validation errors use `CliError` (defined in `error.rs`, derived with `thiserror`). Runtime errors from `lex-data` and `lex-core` propagate as `anyhow::Result`.

## Reports

`simulate -o report.json` writes schema v1: `run` (seed, timestamp, first guess, `strategy` with a `mode` of `unconstrained`, `hard` or `ultrahard`; the older `hard_mode` bool reads as ultrahard or unconstrained; reports without a `strategy` read as ultrahard, T=5, win weight 0), `corpus` (ngrams sha256, dict provenance), `summary` (histogram, mean, failure rate under `uniform` and `prior` weighting), and per-target `games` with the full guess sequence and prior, and `profile` (`serde(default)`, absent for a Corpus run: name, list checksums, counts, Unseen-word floor; `summary.uniform` is its headline). Failure means more than 6 guesses; the solver still plays to completion so the true count is recorded. The prior weighting renormalizes over the targets played: exact for a full-dictionary run, a self-normalized estimate for a sample. `lex compare baseline.json candidate.json` pairs targets by word and warns when corpus checksums differ; it prints the strategies when they differ, and hard-errors when the reports played different Profiles or only one played a Profile.
