# Three-crate workspace

The workspace stays `lex-data` (acquisition, validation, word types), `lex-core` (pure solver, no I/O), and `lex-cli` (the binary). New front ends, such as a TUI, are new crates over `lex-core` rather than features inside `lex-cli`, so the solver stays free of I/O and UI dependencies.
