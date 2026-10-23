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
    /// Paired comparison of two simulation reports (JSON from `simulate --output`)
    Compare(CompareArgs),
    /// Delete cached corpora so they are rebuilt on next use (the Wiktionary dictionary is kept)
    Clear(ClearArgs),
}

#[derive(clap::Args, Debug)]
pub struct ClearArgs {
    #[arg(long, help = "Language whose corpus cache to delete")]
    pub lang: Language,
    #[arg(
        short = 'l',
        long,
        value_parser = parse_word_length,
        help = "Delete only this word length (default: every length for the language)"
    )]
    pub word_length: Option<usize>,
    #[arg(
        long,
        default_value = "data",
        help = "Directory containing cached word-frequency files"
    )]
    pub data_dir: PathBuf,
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
    #[arg(
        short = 'n',
        long,
        help = "Number of target words to sample (default: every word in the dictionary)"
    )]
    pub num_games: Option<usize>,
    #[arg(
        long,
        default_value_t = 0,
        help = "Seed for sampling targets; the same seed and corpus give the same targets"
    )]
    pub seed: u64,
    #[arg(
        long,
        help = "File of target words (one per line) to play instead of sampling"
    )]
    pub targets: Option<PathBuf>,
    #[arg(
        short = 'o',
        long,
        help = "Write the full JSON report here ('-' for stdout, which moves the summary to stderr)"
    )]
    pub output: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
pub struct CompareArgs {
    /// Baseline report
    pub baseline: PathBuf,
    /// Candidate report, played against the same targets
    pub candidate: PathBuf,
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
