# Four-crate workspace

The workspace is `lex-data` (acquisition, validation, word types), `lex-core` (pure solver, no I/O), `lex-tui` (the terminal UI over `lex-core`, launched by the `lex` binary) and `lex-cli` (the binary). A new front end is a new crate over `lex-core` rather than a feature inside `lex-cli`, so the solver stays free of I/O and UI dependencies. A front end library never depends on `lex-cli`: it states what it needs from game setup as a trait, and `lex-cli`, which depends on it, implements that trait over its own setup.

The filename keeps its original number and slug so links to it stay valid.
