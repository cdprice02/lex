use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lex_data::Language;

use crate::error::CliError;
use lex_data::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Simulate batch games against the corpus
    Simulate(SimulateArgs),
    /// Interactively assist a live Wordle game
    Assist(AssistArgs),
}

/// Arguments shared by every subcommand.
#[derive(clap::Args, Debug)]
pub struct CommonArgs {
    #[arg(
        short = 'l',
        long,
        default_value = "5",
        value_parser = parse_word_length,
        help = format!("Length of the words to use in the game ({MIN_WORD_LENGTH}–{MAX_WORD_LENGTH})")
    )]
    pub word_length: usize,
    #[arg(
        long,
        help = "Number of the words to allow in the guesser's dictionary (default: all words in the corpus)"
    )]
    pub dictionary_length: Option<usize>,
    #[arg(
        long,
        default_value = "english",
        help = "Language corpus (english/eng, french/fre, german/ger, spanish/spa, italian/ita, russian/rus)"
    )]
    pub lang: Language,
    #[arg(
        long,
        default_value = "data",
        help = "Directory containing cached word-frequency files"
    )]
    pub data_dir: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct SimulateArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(short = 'n', long, help = "Number of games to simulate")]
    pub num_games: Option<usize>,
}

#[derive(clap::Args, Debug)]
pub struct AssistArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(
        short = 's',
        long,
        default_value_t = 5,
        help = "Number of suggestions to show per turn"
    )]
    pub suggestions: usize,
}

fn parse_word_length(s: &str) -> anyhow::Result<usize> {
    let n: usize = s.parse()?;
    if (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&n) {
        Ok(n)
    } else {
        Err(CliError::InvalidWordLength {
            range: MIN_WORD_LENGTH..=MAX_WORD_LENGTH,
            got: n,
        }
        .into())
    }
}
