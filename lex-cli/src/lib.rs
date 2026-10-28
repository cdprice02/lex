//! The `lex` command line as a library, so the binary and the tests drive the same code. Internal:
//! nothing here is a stable API.

pub mod assist;
pub mod cli;
pub mod compare;
pub mod config;
pub mod error;
pub mod lists;
pub mod report;
pub mod simulate;
pub mod targets;
pub mod tracks;
