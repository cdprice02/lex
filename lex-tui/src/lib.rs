//! The terminal UI of lex. A frontend library: the `lex` binary launches it, and it never
//! depends on `lex-cli`.
//!
//! Watch is the first mode: lex plays a hidden answer one guess at a time, explaining each guess.
//! The state is one [`Model`] changed by [`Msg`]s through the pure [`update`]; slow work leaves as
//! a [`Request`] for a worker that owns the [`Session`].
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

#[cfg(test)]
mod fake;
mod model;
mod runtime;
mod session;
mod view;

pub use model::{Display, Entry, Model, Msg, Options, Origin, Phase, Request, Symbols, update};
pub use runtime::{drive, enter, handle, press, run};
pub use session::{ExplanationView, GameInfo, Played, Row, Session};
pub use view::{MIN_HEIGHT, MIN_WIDTH, draw};
