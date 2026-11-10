//! The `lex` binary: hands a parsed command line to its subcommand.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

use std::num::NonZeroUsize;
use std::process::ExitCode;

use lex_cli::assist::assist;
use lex_cli::book_listing::{self, show_game};
use lex_cli::books::build_command;
use lex_cli::clear;
use lex_cli::cli::{
    BookCommand, Cli, Command, CorpusCommand, DictCommand, ProfileCommand, build_pool,
};
use lex_cli::compare::compare_reports;
use lex_cli::corpora;
use lex_cli::dicts;
use lex_cli::profiles;
use lex_cli::progress;
use lex_cli::simulate::simulate;
use lex_cli::style::{self, SymbolScheme};
use lex_cli::word;

fn main() -> ExitCode {
    let cli = Cli::parse_args(std::env::args_os()).unwrap_or_else(|e| e.exit());
    let download_request = cli.download_request();
    style::init(cli.color);
    progress::init(cli.no_progress);
    let result = build_pool(cli.threads)
        .map_err(anyhow::Error::from)
        .and_then(|pool| pool.install(|| run(cli.command, cli.symbols, download_request)));
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `command` and returns the process exit code.
fn run(
    command: Command,
    symbols: SymbolScheme,
    download_workers: Option<NonZeroUsize>,
) -> anyhow::Result<ExitCode> {
    let done = |result: anyhow::Result<()>| result.map(|()| ExitCode::SUCCESS);
    match command {
        Command::Simulate(mut args) => {
            args.common.download_workers = download_workers;
            done(lex_data::match_word_length!(
                simulate,
                args.dispatch_length(),
                &args
            ))
        }
        Command::Assist(mut args) => {
            args.symbols = symbols;
            args.common.download_workers = download_workers;
            done(lex_data::match_word_length!(
                assist,
                args.common.dispatch_length(),
                &args
            ))
        }
        Command::Compare(args) => done(compare_reports(&args)),
        Command::Verdict(args) => Ok(lex_cli::verdict::command(&args)),
        Command::Word(args) => done(word::run(&args)),
        Command::Book(args) => match args.command {
            BookCommand::Build(mut args) => {
                args.common.download_workers = download_workers;
                done(lex_data::match_word_length!(
                    build_command,
                    args.dispatch_length(),
                    &args
                ))
            }
            BookCommand::List(args) => done(book_listing::list(&args)),
            BookCommand::Show(args) => done(if args.id.is_some() {
                book_listing::show_id(&args)
            } else {
                lex_data::match_word_length!(show_game, args.common.dispatch_length(), &args)
            }),
            BookCommand::Clear(args) => done(clear::book(&args)),
        },
        Command::Dict(args) => match args.command {
            DictCommand::List(args) => done(dicts::list(&args)),
            DictCommand::Show(args) => done(dicts::show(&args)),
            DictCommand::Clear(args) => done(clear::dict(&args)),
        },
        Command::Corpus(args) => match args.command {
            CorpusCommand::List(args) => done(corpora::list(&args)),
            CorpusCommand::Show(args) => done(corpora::show(&args)),
            CorpusCommand::Clear(args) => done(clear::corpus(&args)),
        },
        Command::Profile(args) => match args.command {
            ProfileCommand::List(args) => done(profiles::list(&args)),
            ProfileCommand::Show(args) => done(profiles::show(&args)),
            ProfileCommand::Clear(args) => done(clear::profile(&args)),
        },
    }
}
