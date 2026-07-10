use std::path::PathBuf;

use clap::Parser;
use lex_data::Language;

use crate::error::CliError;
use lex_data::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

// TODO(.tasks/01-assist-mode.md): split into subcommands — simulate (default) | assist,
// with a dict inspection subcommand later (ROADMAP.md "Unscheduled").
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
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
        help = format!("Number of the words to allow in the guesser's dictionary (default: all words in the corpus)")
    )]
    pub dictionary_length: Option<usize>,
    #[arg(short = 'n', long, help = "Number of games to simulate")]
    pub num_games: Option<usize>,
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
