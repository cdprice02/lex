//! The `lex` binary: hands a parsed command line to its subcommand.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

use std::process::ExitCode;

use clap::Parser;

use lex_cli::assist::assist;
use lex_cli::books::build_command;
use lex_cli::clear::clear;
use lex_cli::cli::{BookCommand, Cli, Command};
use lex_cli::compare::compare_reports;
use lex_cli::simulate::simulate;

fn main() -> ExitCode {
    env_logger::init();

    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `command` and returns the process exit code.
fn run(command: Command) -> anyhow::Result<ExitCode> {
    let done = |result: anyhow::Result<()>| result.map(|()| ExitCode::SUCCESS);
    match command {
        Command::Simulate(args) => done(lex_data::match_word_length!(
            simulate,
            args.dispatch_length(),
            &args
        )),
        Command::Assist(args) => done(lex_data::match_word_length!(
            assist,
            args.common.dispatch_length(),
            &args
        )),
        Command::Compare(args) => done(compare_reports(&args)),
        Command::Verdict(args) => Ok(lex_cli::verdict::command(&args)),
        Command::Book(args) => match args.command {
            BookCommand::Build(args) => done(lex_data::match_word_length!(
                build_command,
                args.dispatch_length(),
                &args
            )),
        },
        Command::Clear(args) => done(clear(&args)),
    }
}
