use clap::Parser;

use lex_cli::assist::assist;
use lex_cli::cli::{Cli, Command};
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
