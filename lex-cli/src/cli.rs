use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lex_core::{GuessPool, Strategy};
use lex_data::{Language, Profile};

use crate::config::{Game, RunConfig, Targets};
use crate::error::CliError;
use crate::tracks::Track;
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
        default_value_t = Strategy::DEFAULT.pool,
        help = "Which words may be guessed: all (every word), hard (the official Wordle hard-mode rule: keep every green in place, reuse every green or yellow letter) or ultrahard (only words consistent with every feedback so far, stricter than hard)"
    )]
    pub pool: GuessPool,
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.temperature,
        value_parser = parse_temperature,
        help = "Answer prior temperature: higher flattens the frequency-based prior"
    )]
    pub temperature: f64,
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.win_weight,
        value_parser = parse_win_weight,
        help = "Score bonus, in bits, per unit of probability that a guess is the answer"
    )]
    pub win_weight: f64,
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.endgame_threshold,
        help = "Search the game tree exactly once this few candidates remain (0 disables)"
    )]
    pub endgame_threshold: usize,
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.endgame_aversion,
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

    pub fn game(&self) -> Game {
        match self.profile {
            Some(profile) => Game::Profile(profile),
            None => Game::Corpus {
                language: self.lang.expect("clap requires --lang without --profile"),
                length: self.dispatch_length(),
                dictionary_length: self.dictionary_length,
            },
        }
    }

    pub fn strategy(&self) -> Strategy {
        Strategy {
            pool: self.pool,
            temperature: self.temperature,
            win_weight: self.win_weight,
            endgame_threshold: self.endgame_threshold,
            endgame_aversion: self.endgame_aversion,
        }
    }

    pub fn config(&self, targets: Targets) -> RunConfig {
        RunConfig {
            game: self.game(),
            strategy: self.strategy(),
            targets,
        }
    }
}

#[derive(clap::Args, Debug)]
#[command(
    mut_arg("lang", |a| a.required_unless_present("track")),
    mut_arg("word_length", |a| a.required_unless_present("track"))
)]
pub struct SimulateArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(
        short = 'n',
        long,
        conflicts_with = "targets",
        help = "Number of target words to sample (default: every word in the dictionary)"
    )]
    pub num_games: Option<usize>,
    #[arg(
        long,
        default_value_t = 0,
        requires = "num_games",
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
    #[arg(
        long,
        hide = true,
        value_parser = parse_track,
        conflicts_with_all = [
            "lang", "word_length", "dictionary_length", "profile", "pool", "temperature",
            "win_weight", "endgame_threshold", "endgame_aversion", "num_games", "seed", "targets",
        ],
        help = "Play a benchmark Track by id (its game, strategy and targets)"
    )]
    pub track: Option<Track>,
}

impl SimulateArgs {
    /// The length to monomorphize over.
    pub fn dispatch_length(&self) -> usize {
        match self.track {
            Some(track) => track.game.word_length(),
            None => self.common.dispatch_length(),
        }
    }

    /// What these arguments ask to play.
    pub fn config(&self) -> RunConfig {
        if let Some(track) = self.track {
            return track.config();
        }
        let targets = match (&self.targets, self.num_games) {
            (Some(path), _) => Targets::File(path.clone()),
            (None, Some(n)) => Targets::Sample { n, seed: self.seed },
            (None, None) => Targets::Full,
        };
        self.common.config(targets)
    }
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

fn parse_track(s: &str) -> anyhow::Result<Track> {
    Track::find(s).ok_or_else(|| CliError::UnknownTrack(s.to_owned()).into())
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
    fn a_profile_plays_every_pool() {
        for (flag, pool) in [
            ("all", GuessPool::All),
            ("hard", GuessPool::Hard),
            ("ultrahard", GuessPool::Ultrahard),
        ] {
            let common = common(&["--profile", "wordle", "--pool", flag]).unwrap();
            assert_eq!(common.strategy().pool, pool);
        }
    }

    #[test]
    fn the_pool_defaults_to_all_and_the_old_flags_are_gone() {
        assert_eq!(
            common(&["--profile", "wordle"]).unwrap().strategy().pool,
            GuessPool::All
        );
        for old in ["--hard", "--ultrahard", "--hard-mode"] {
            assert!(common(&["--profile", "wordle", old]).is_err(), "{old}");
        }
        assert!(common(&["--profile", "wordle", "--pool", "easy"]).is_err());
    }
}

#[cfg(test)]
mod track_tests {
    use super::*;
    use crate::config::Targets;

    fn simulate(args: &[&str]) -> Result<SimulateArgs, clap::Error> {
        match Cli::try_parse_from(["lex", "simulate"].into_iter().chain(args.iter().copied()))?
            .command
        {
            Command::Simulate(a) => Ok(a),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_track_stands_alone_and_plays_its_config() {
        let args = simulate(&["--track", "wordle-hard"]).unwrap();
        assert_eq!(args.config(), Track::find("wordle-hard").unwrap().config());
        assert_eq!(args.dispatch_length(), 5);
        assert_eq!(
            simulate(&["--track", "russian-8"])
                .unwrap()
                .dispatch_length(),
            8
        );
    }

    #[test]
    fn a_track_conflicts_with_game_pool_and_target_flags() {
        for extra in [
            &["--lang", "english"][..],
            &["-l", "5"],
            &["--profile", "wordle"],
            &["--pool", "hard"],
            &["--temperature", "2"],
            &["-n", "10"],
            &["--seed", "1"],
            &["--targets", "t.txt"],
        ] {
            let args: Vec<&str> = ["--track", "english-5"]
                .into_iter()
                .chain(extra.iter().copied())
                .collect();
            assert!(simulate(&args).is_err(), "{extra:?}");
        }
        assert!(simulate(&["--track", "english-5", "-o", "-"]).is_ok());
        assert!(simulate(&["--track", "english-5ultrahard"]).is_err());
    }

    #[test]
    fn flags_become_a_run_config() {
        let sampled =
            simulate(&["-l", "5", "--lang", "english", "-n", "7", "--seed", "3"]).unwrap();
        assert_eq!(sampled.config().targets, Targets::Sample { n: 7, seed: 3 });
        let full = simulate(&["-l", "5", "--lang", "english", "--pool", "hard"]).unwrap();
        assert_eq!(full.config().targets, Targets::Full);
        assert_eq!(full.config().strategy.pool, GuessPool::Hard);
        let file = simulate(&["-l", "5", "--lang", "english", "--targets", "t.txt"]).unwrap();
        assert_eq!(file.config().targets, Targets::File("t.txt".into()));
        assert!(simulate(&["-l", "5", "--lang", "english", "--targets", "t", "-n", "1"]).is_err());
        assert!(simulate(&["-l", "5", "--lang", "english", "--seed", "1"]).is_err());
    }
}
