//! The `lex` command line as a library, so the binary and the tests drive the same code. Internal:
//! nothing here is a stable API.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

pub mod assist;
pub mod book_listing;
pub mod books;
pub mod clear;
pub mod cli;
pub mod compare;
pub mod config;
pub mod dicts;
pub mod error;
pub mod lists;
pub mod progress;
pub mod report;
pub mod simulate;
pub mod style;
pub mod targets;
pub mod tracks;
pub mod verdict;
