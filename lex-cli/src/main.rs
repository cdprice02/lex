use clap::Parser;

use assist::assist;
use cli::{Cli, Command};
use compare::compare_reports;
use simulate::simulate;

mod assist;
mod cli;
mod compare;
mod error;
mod report;
mod simulate;
mod targets;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    match Cli::parse().command {
        Command::Simulate(args) => {
            lex_data::match_word_length!(simulate, args.common.word_length, &args)
        }
        Command::Assist(args) => {
            lex_data::match_word_length!(assist, args.common.word_length, &args)
        }
        Command::Compare(args) => compare_reports(&args),
        Command::Clear(args) => {
            lex_data::blocking::DataDir::new(&args.data_dir).clear(args.lang, args.word_length)
        }
    }
}
