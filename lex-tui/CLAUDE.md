# CLAUDE.md: lex-tui

The terminal UI of lex: a library (`lex_tui`) that `lex-cli` launches as `lex tui`. It depends on `lex-core` for `Correctness` and never on `lex-cli`; `lex-cli` depends on it and implements `Session` over its own game setup. Watch is its mode: lex plays a hidden answer and explains each guess. Per-function detail lives in rustdoc.

## Module map

| Module | Owns |
|---|---|
| `session.rs` | the `Session` trait, its `SessionError`, and its plain types: `Played`, `GameInfo`, `ExplanationView`, `Row` |
| `model.rs` | `Model`, `Msg`, `Request`, `Options`, and the pure `update` (every key and every worker answer) |
| `view.rs` | `draw(frame, &Model)`: board left, explanation right, two-line status bar, the `/` box, the too-small notice |
| `runtime.rs` | `run` (terminal loop plus worker thread), `handle` (one `Request` to one `Msg`), and for tests `drive` (headless play), `press` and `enter` |
| `fake.rs` | test-only scripted `Session`, and `Broken`, which fails one chosen call |

## Invariants and gotchas

- **One copy.** The trait speaks `String`, `Vec<Correctness>` and view structs, and `run` takes a `Box<dyn Session + Send>`: nothing here is generic over the word length, so the crate compiles once (ADR 0010). Keep `N` out of it.
- **Public surface.** Only `run`, `Options`, `Symbols`, `Session` and its types are public. The `test-support` feature adds the model, `update`, `handle`, `drive`, `press`, `enter` and `draw` for `lex-cli`'s headless test (a dev-dependency turns it on); inside this crate they are always there under `cfg(test)`.
- **Errors.** Every `Session` call returns `SessionError`. `update` records a failure in `Model::error` (the status bar shows it) and moves to `Phase::Failed`, from which `n` starts another game; a failed `/` check shows in the box instead. A failure tagged with an abandoned game's epoch is dropped like any stale answer.
- **`update` is pure.** It never sleeps, spawns or reads the terminal. Anything slow or fallible leaves as a `Request`; the answer returns as a `Msg`. `handle` is the only code that calls the `Session`, and both the worker thread and `drive` use it, so tests and the terminal run one path.
- **Epochs.** Each game bumps `Model::epoch`; `Feedback` and `Explained` carry the epoch they were asked in and are dropped on a mismatch, as is a `Drawn` for a seed other than the one pending. `n` therefore abandons a solve in flight without cancelling it.
- **The worker is detached.** `run` never joins it: quitting mid-solve returns at once and the process exit ends it. The `explain` call runs inside the `--threads` pool (that is `lex-cli`'s `Session`, not this crate); the UI loop runs outside it.
- **Seeds.** The first game uses `Options::seed`; `n` uses the previous seed plus one, so a fixed `--seed` replays the same sequence of games.
- **Typed targets** are checked by the session (`check_target`) on Enter, so the `/` box stays open with the reason when the word is not an answer. It takes letters only.
- **Colour.** The palette is the three tile colours in `view.rs`, a copy of `lex-cli`'s `style` palette (this crate cannot depend on it). With colour off no style is applied and each tile also shows its symbol (`s` cycles letters, digits, emoji); with colour on the symbol scheme has no tile to change.
- **Size.** The layout is for 80x24; below that `draw` shows only "terminal too small (needs 80x24)". The board widens with the word length and tiles past the board's width are clipped.

## Tests

`update` is tested key by key against `fake::Fake` in `model.rs`. Screens are insta snapshots of a `TestBackend` at 80x24 with colour off (`src/snapshots/`), which assert text and not styles. Each fallible call has a test against `Broken`. The real `Session` plays a whole game headless in `lex-cli/tests/tui.rs`. Nothing starts a terminal.
