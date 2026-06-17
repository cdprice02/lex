use std::path::PathBuf;

use clap::Parser;
use lex_core::error::LexError;
use lex_data::Language;
use lex_data::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

// TODO: implement subcommands for different modes (e.g. interactive mode, simulation mode, etc.)
// TODO: implement subcommands for listing words, frequencies, etc. for debugging and exploration purposes
// TODO: implement subcommands for listing options of parameters like language, word length, etc.
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
        Err(LexError::UnexpectedWordLength {
            range: MIN_WORD_LENGTH..=MAX_WORD_LENGTH,
            got: n,
        }
        .into())
    }
}
