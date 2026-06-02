// TODO: add benches for important functions like cache read/write, word parsing, guesser next_guess, etc.
// TODO: add more comments and documentation for functions, structs, etc. to improve code readability and maintainability
// TODO: add more tests for edge cases, error handling, etc. (unit, integration, prop, etc.)
use clap::Parser;

use lex_core::cli::Args;
use lex_core::match_word_length_run;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match_word_length_run!(&args)?;

    Ok(())
}
