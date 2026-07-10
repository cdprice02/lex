use clap::Parser;

use cli::Args;

mod cli;
mod error;
#[macro_use]
mod simulate;

configure_word_length_bounds!(3, 10);

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match_word_length_run!(&args)
}
