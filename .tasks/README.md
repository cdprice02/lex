# Task briefs

One file per roadmap phase (see `../ROADMAP.md` for ordering, rationale, and
standing decisions). Each brief carries enough context to be planned and
executed independently, but deliberately leaves implementation choices open
where noted.

## For the executing agent

1. Read `ROADMAP.md` (ordering, decisions, and the scope constraint: lex is a
   generic solver — no English-only or 5-letter-only assumptions), then this
   task's brief, then the `CLAUDE.md` of every crate you touch.
2. Check `Depends on:` — do not start a task whose dependencies have not
   landed.
3. `Open questions` are deliberate degrees of freedom. Propose a concrete
   choice and confirm with the user before building on it; never silently
   pick.
4. `Out of scope` items are covered by other task briefs — do not drift into
   them, even when adjacent.
5. Before declaring done: `cargo test --all`, `cargo clippy --all
   --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`,
   plus the brief's own `Done when` criteria. The user makes commits; signal
   what to commit and when.
6. Solver-behavior changes (tasks 04, 05, 08) must include a paired
   per-target before/after comparison against the committed baselines from
   task 02.
