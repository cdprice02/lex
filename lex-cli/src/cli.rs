use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lex_core::{GuessPool, Strategy};
use lex_data::{Language, Profile};

use crate::bench::Tier;
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
    /// Read or recalibrate the Suite manifest (`benchmarks/suite.json`)
    Bench(BenchArgs),
    /// Delete a cached corpus or a fetched Profile so it is rebuilt on next use (the Wiktionary dictionary is kept)
    Clear(ClearArgs),
}

#[derive(clap::Args, Debug)]
pub struct BenchArgs {
    #[arg(
        long,
        conflicts_with = "calibrate",
        help = "Check the manifest against the generated matrix and print each Track's simulate command; needs no corpus data"
    )]
    pub dry_run: bool,
    #[arg(
        long,
        help = "Run each Track on one solver thread, choose its targets, record CPU and wall time, and rewrite the manifest after each"
    )]
    pub calibrate: bool,
    #[arg(
        long,
        help = "Select this Track by id (repeatable); default every Track"
    )]
    pub track: Vec<String>,
    #[arg(long, value_enum, help = "Select only the Tracks of this Tier")]
    pub tier: Option<Tier>,
    #[arg(
        long,
        help = "Calibrate on a loaded machine; the Track records contended: true"
    )]
    pub force: bool,
    #[arg(
        long,
        default_value = "benchmarks/suite.json",
        help = "The Suite manifest"
    )]
    pub manifest: PathBuf,
    #[arg(
        long,
        default_value = "data",
        help = "Directory containing cached word-frequency files"
    )]
    pub data_dir: PathBuf,
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
        value_parser = parse_word_length,
        required_unless_present = "profile",
        conflicts_with = "profile",
        help = format!("Length of the words to use in the game ({MIN_WORD_LENGTH}–{MAX_WORD_LENGTH}); a profile fixes it")
    )]
    pub word_length: Option<usize>,
    #[arg(
        long,
        conflicts_with = "profile",
        help = "Number of the words to allow in the guesser's dictionary (default: all words in the corpus)"
    )]
    pub dictionary_length: Option<usize>,
    #[arg(
        long,
        required_unless_present = "profile",
        conflicts_with = "profile",
        help = "Language corpus (english/eng, french/fre, german/ger, spanish/spa, italian/ita, russian/rus); a profile fixes it"
    )]
    pub lang: Option<Language>,
    #[arg(
        long,
        help = "Play a profile: its answer list and guess pool, with its language and word length (wordle, wordle-all)"
    )]
    pub profile: Option<Profile>,
    #[arg(
        long,
        default_value = "data",
        help = "Directory containing cached word-frequency files"
    )]
    pub data_dir: PathBuf,
    #[arg(
        long,
        conflicts_with = "ultrahard",
        help = "Official Wordle hard mode: keep every green in place and reuse every green or yellow letter"
    )]
    pub hard: bool,
    #[arg(
        long,
        help = "Only guess words consistent with every feedback so far (stricter than --hard)"
    )]
    pub ultrahard: bool,
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
    /// The word length to monomorphize over: the profile's, else `-l` (clap requires one).
    pub fn dispatch_length(&self) -> usize {
        match self.profile {
            Some(profile) => profile.spec().word_length,
            None => self
                .word_length
                .expect("clap requires -l without --profile"),
        }
    }

    pub fn strategy(&self) -> Strategy {
        Strategy {
            pool: if self.ultrahard {
                GuessPool::Ultrahard
            } else if self.hard {
                GuessPool::Hard
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

#[cfg(test)]
mod profile_tests {
    use super::*;

    fn common(args: &[&str]) -> Result<CommonArgs, clap::Error> {
        match Cli::try_parse_from(["lex", "assist"].into_iter().chain(args.iter().copied()))?
            .command
        {
            Command::Assist(a) => Ok(a.common),
            _ => unreachable!(),
        }
    }

    #[test]
    fn language_and_length_are_required_without_a_profile() {
        assert!(common(&[]).is_err());
        assert!(common(&["--lang", "english"]).is_err());
        assert!(common(&["-l", "5"]).is_err());
        assert!(common(&["--lang", "english", "-l", "5"]).is_ok());
    }

    #[test]
    fn a_profile_stands_alone() {
        assert!(common(&["--profile", "wordle"]).is_ok());
    }

    #[test]
    fn a_profile_rejects_language_and_length_even_when_they_agree() {
        assert!(common(&["--profile", "wordle", "-l", "5"]).is_err());
        assert!(common(&["--profile", "wordle", "-l", "6"]).is_err());
        assert!(common(&["--profile", "wordle", "--lang", "english"]).is_err());
    }

    #[test]
    fn a_profile_rejects_dictionary_length() {
        assert!(common(&["--profile", "wordle", "--dictionary-length", "10"]).is_err());
    }

    #[test]
    fn a_profile_plays_hard_and_ultrahard() {
        let hard = common(&["--profile", "wordle", "--hard"]).unwrap();
        assert_eq!(hard.strategy().pool, GuessPool::Hard);
        let ultra = common(&["--profile", "wordle", "--ultrahard"]).unwrap();
        assert_eq!(ultra.strategy().pool, GuessPool::Ultrahard);
    }

    #[test]
    fn hard_and_ultrahard_conflict_and_the_old_flag_is_gone() {
        assert!(common(&["--profile", "wordle", "--hard", "--ultrahard"]).is_err());
        assert!(common(&["--profile", "wordle", "--hard-mode"]).is_err());
        assert_eq!(
            common(&["--profile", "wordle"]).unwrap().strategy().pool,
            GuessPool::All
        );
    }
}
