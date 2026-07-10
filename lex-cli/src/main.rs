use clap::Parser;

use assist::assist;
use cli::{Cli, Command};
use simulate::simulate;

mod assist;
mod cli;
mod error;
mod simulate;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    match Cli::parse().command {
        Command::Simulate(args) => {
            lex_data::match_word_length!(simulate, args.common.word_length, &args)
        }
        Command::Assist(args) => {
            lex_data::match_word_length!(assist, args.common.word_length, &args)
        }
    }
}
