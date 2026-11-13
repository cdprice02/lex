//! The terminal UI of lex. A frontend library: the `lex` binary launches it, and it never
//! depends on `lex-cli`.
//!
//! Watch is its mode: lex plays a hidden answer one guess at a time, explaining each guess.
//! The state is one model changed by messages through a pure update; slow work leaves as a request
//! for a worker that owns the [`Session`]. The `test-support` feature exposes that core, and a
//! headless driver, to other crates' tests.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

#[cfg(test)]
mod fake;
mod model;
mod runtime;
mod session;
mod view;

pub use model::{Options, Symbols};
pub use runtime::run;
pub use session::{ExplanationView, GameInfo, Played, Row, Session, SessionError};

#[cfg(feature = "test-support")]
pub use model::{Display, Entry, Model, Msg, Origin, Phase, Request, update};
#[cfg(feature = "test-support")]
pub use runtime::{drive, enter, handle, press};
#[cfg(feature = "test-support")]
pub use view::{MIN_HEIGHT, MIN_WIDTH, draw};
