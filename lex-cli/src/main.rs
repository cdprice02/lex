//! The `lex` binary: hands a parsed command line to its subcommand.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

use std::process::ExitCode;

use clap::Parser;

use lex_cli::assist::assist;
use lex_cli::books::build_command;
use lex_cli::clear;
use lex_cli::cli::{
    BookCommand, Cli, Command, CorpusCommand, DictCommand, ProfileCommand, build_pool,
};
use lex_cli::compare::compare_reports;
use lex_cli::dicts;
use lex_cli::progress;
use lex_cli::simulate::simulate;
use lex_cli::style::{self, SymbolScheme};

fn main() -> ExitCode {
    let cli = Cli::parse();
    style::init(cli.color);
    progress::init(cli.no_progress);
    let result = build_pool(cli.threads)
        .map_err(anyhow::Error::from)
        .and_then(|pool| pool.install(|| run(cli.command, cli.symbols)));
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `command` and returns the process exit code.
fn run(command: Command, symbols: SymbolScheme) -> anyhow::Result<ExitCode> {
    let done = |result: anyhow::Result<()>| result.map(|()| ExitCode::SUCCESS);
    match command {
        Command::Simulate(args) => done(lex_data::match_word_length!(
            simulate,
            args.dispatch_length(),
            &args
        )),
        Command::Assist(mut args) => {
            args.symbols = symbols;
            done(lex_data::match_word_length!(
                assist,
                args.common.dispatch_length(),
                &args
            ))
        }
        Command::Compare(args) => done(compare_reports(&args)),
        Command::Verdict(args) => Ok(lex_cli::verdict::command(&args)),
        Command::Book(args) => match args.command {
            BookCommand::Build(args) => done(lex_data::match_word_length!(
                build_command,
                args.dispatch_length(),
                &args
            )),
            BookCommand::Clear(args) => done(clear::book(&args)),
        },
        Command::Dict(args) => match args.command {
            DictCommand::List(args) => done(dicts::list(&args)),
            DictCommand::Show(args) => done(dicts::show(&args)),
            DictCommand::Clear(args) => done(clear::dict(&args)),
        },
        Command::Corpus(args) => match args.command {
            CorpusCommand::Clear(args) => done(clear::corpus(&args)),
        },
        Command::Profile(args) => match args.command {
            ProfileCommand::Clear(args) => done(clear::profile(&args)),
        },
    }
}
