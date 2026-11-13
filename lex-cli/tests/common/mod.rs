//! Helpers the command-parsing integration tests share.
#![expect(
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::path::Path;

use clap::Parser;
use lex_cli::cli::{Cli, Command};

/// Parses `args` after `lex` and returns the command.
pub fn parse(args: &[&str]) -> Result<Command, clap::Error> {
    Cli::try_parse_from(std::iter::once("lex").chain(args.iter().copied())).map(|cli| cli.command)
}

/// Writes `root/<relative>` with a stand-in body.
pub fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}
