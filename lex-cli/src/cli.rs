use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lex_core::{GuessPool, Strategy};
use lex_data::{Language, Profile};

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
    /// Delete a cached corpus or a fetched Profile so it is rebuilt on next use (the Wiktionary dictionary is kept)
    Clear(ClearArgs),
}

#[derive(clap::Args, Debug)]
pub struct ClearArgs {
    #[arg(
        long,
        required_unless_present = "profile",
        conflicts_with = "profile",
        help = "Language whose corpus cache to delete"
    )]
    pub lang: Option<Language>,
    #[arg(
        long,
        help = "Profile whose fetched lists to delete (never touches corpora)"
    )]
    pub profile: Option<Profile>,
    #[arg(
        short = 'l',
        long,
        conflicts_with = "profile",
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
    #[arg(
        long,
        help = "Only guess words that are still possible answers (the original solver)"
    )]
    pub hard_mode: bool,
    #[arg(
        long,
        default_value_t = Strategy::default().temperature,
        value_parser = parse_temperature,
        help = "Answer prior temperature: higher flattens the frequency-based prior"
    )]
    pub temperature: f64,
    #[arg(
        long,
        default_value_t = Strategy::default().win_weight,
        value_parser = parse_win_weight,
        help = "Score bonus, in bits, per unit of probability that a guess is the answer"
    )]
    pub win_weight: f64,
    #[arg(
        long,
        default_value_t = Strategy::default().endgame_threshold,
        help = "Search the game tree exactly once this few candidates remain (0 disables)"
    )]
    pub endgame_threshold: usize,
    #[arg(
        long,
        default_value_t = Strategy::default().endgame_aversion,
        value_parser = parse_win_weight,
        help = "Endgame risk aversion: minimize the mean of e^(aversion x guesses); 0 minimizes plain expected guesses"
    )]
    pub endgame_aversion: f64,
}

impl CommonArgs {
    pub fn strategy(&self) -> Strategy {
        Strategy {
            pool: if self.hard_mode {
                GuessPool::Candidates
            } else {
                GuessPool::All
            },
            temperature: self.temperature,
            win_weight: self.win_weight,
            endgame_threshold: self.endgame_threshold,
            endgame_aversion: self.endgame_aversion,
        }
    }
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

fn parse_temperature(s: &str) -> anyhow::Result<f64> {
    let t: f64 = s.parse()?;
    if t.is_finite() && t > 0.0 {
        Ok(t)
    } else {
        Err(CliError::InvalidTemperature(t).into())
    }
}

fn parse_win_weight(s: &str) -> anyhow::Result<f64> {
    let w: f64 = s.parse()?;
    if w.is_finite() && w >= 0.0 {
        Ok(w)
    } else {
        Err(CliError::InvalidWinWeight(w).into())
    }
}

#[cfg(test)]
mod clear_tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["lex", "clear"].into_iter().chain(args.iter().copied()))
    }

    #[test]
    fn exactly_one_of_lang_and_profile() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--lang", "english"]).is_ok());
        assert!(parse(&["--profile", "wordle"]).is_ok());
        assert!(parse(&["--lang", "english", "--profile", "wordle"]).is_err());
    }

    #[test]
    fn word_length_goes_only_with_lang() {
        assert!(parse(&["--lang", "english", "-l", "5"]).is_ok());
        assert!(parse(&["--profile", "wordle", "-l", "5"]).is_err());
    }
}
