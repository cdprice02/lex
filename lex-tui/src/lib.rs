//! The terminal UI of lex. A frontend library: the `lex` binary launches it, and it never
//! depends on `lex-cli`.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

mod session;

pub use session::{ExplanationView, GameInfo, Played, Row, Session};
