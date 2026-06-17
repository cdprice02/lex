// TODO: add more comments and documentation for functions, structs, etc. to improve code readability and maintainability (include doctests where appropriate)
// TODO: add module comments to explain the purpose and functionality of each module and how they interact with each other
// TODO: add doc comment required for public items in all crates to enforce documentation standards and improve code readability
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
