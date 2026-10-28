use clap::Parser;

use assist::assist;
use cli::{Cli, Command};
use compare::compare_reports;
use simulate::simulate;

mod assist;
mod bench;
mod cli;
mod compare;
mod error;
mod lists;
mod report;
mod simulate;
mod targets;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    match Cli::parse().command {
        Command::Simulate(args) => {
            lex_data::match_word_length!(simulate, args.common.dispatch_length(), &args)
        }
        Command::Assist(args) => {
            lex_data::match_word_length!(assist, args.common.dispatch_length(), &args)
        }
        Command::Bench(args) => bench::bench(&args),
        Command::Compare(args) => compare_reports(&args),
        Command::Clear(args) => {
            let dir = lex_data::DataDir::new(&args.data_dir);
            match (args.lang, args.profile) {
                (Some(lang), _) => dir.clear(lang, args.word_length),
                (None, Some(profile)) => dir.clear_profile(profile),
                (None, None) => unreachable!("clap requires --lang or --profile"),
            }
        }
    }
}
