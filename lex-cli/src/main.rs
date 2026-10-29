use clap::Parser;

use lex_cli::assist::assist;
use lex_cli::books::build_command;
use lex_cli::clear::clear;
use lex_cli::cli::{BookCommand, Cli, Command};
use lex_cli::compare::compare_reports;
use lex_cli::simulate::simulate;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    match Cli::parse().command {
        Command::Simulate(args) => {
            lex_data::match_word_length!(simulate, args.dispatch_length(), &args)
        }
        Command::Assist(args) => {
            lex_data::match_word_length!(assist, args.common.dispatch_length(), &args)
        }
        Command::Compare(args) => compare_reports(&args),
        Command::Book(args) => match args.command {
            BookCommand::Build(args) => {
                lex_data::match_word_length!(build_command, args.dispatch_length(), &args)
            }
        },
        Command::Clear(args) => clear(&args),
    }
}
