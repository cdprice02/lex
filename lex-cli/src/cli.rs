//! The command line: clap definitions and their conversion to run configurations.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lex_core::{GuessPool, Strategy};
use lex_data::{Language, Profile};

use crate::config::{Game, RunConfig, Targets};
use crate::error::CliError;
use crate::tracks::Track;
use lex_data::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

/// The `lex` command line.
// `about = None`: a doc comment would otherwise become the `--help` banner.
#[derive(Parser, Debug)]
#[command(version, about = None, long_about = None)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands of `lex`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Simulate batch games against the corpus
    Simulate(SimulateArgs),
    /// Interactively assist a live Wordle game
    Assist(AssistArgs),
    /// Paired comparison of two simulation reports (JSON from `simulate --output`)
    Compare(CompareArgs),
    /// Delete a cached corpus, a fetched Profile or the opening books so they are rebuilt on next use (the Wiktionary dictionary is kept)
    Clear(ClearArgs),
    /// Manage opening book files
    Book(BookArgs),
    /// Judge a head set of Track snapshots against a base set, or refresh the README table
    #[command(hide = true)]
    Verdict(VerdictArgs),
}

/// Arguments of `lex book`.
#[derive(clap::Args, Debug)]
pub struct BookArgs {
    /// The book operation.
    #[command(subcommand)]
    pub command: BookCommand,
}

/// The operations of `lex book`.
#[derive(Subcommand, Debug)]
pub enum BookCommand {
    /// Build the opening book for a game and strategy, or report that it is up to date
    Build(BookBuildArgs),
}

/// Arguments of `lex book build`.
#[derive(clap::Args, Debug)]
#[command(
    mut_arg("lang", |a| a.required_unless_present("track")),
    mut_arg("word_length", |a| a.required_unless_present("track"))
)]
pub struct BookBuildArgs {
    /// The game and Strategy to build for.
    #[command(flatten)]
    pub common: CommonArgs,
    /// Rebuild even when the book is up to date
    #[arg(long)]
    pub force: bool,
    /// Build the book of a benchmark Track by id
    #[arg(
        long,
        hide = true,
        value_parser = parse_track,
        conflicts_with_all = [
            "lang", "word_length", "dictionary_length", "profile", "pool", "temperature",
            "win_weight", "endgame_threshold", "endgame_aversion",
        ]
    )]
    pub track: Option<Track>,
}

impl BookBuildArgs {
    /// The length to monomorphize over.
    #[must_use]
    pub const fn dispatch_length(&self) -> usize {
        match self.track {
            Some(track) => track.game.word_length(),
            None => self.common.dispatch_length(),
        }
    }

    /// What these arguments build the book for. A book covers the full Answer set whatever
    /// targets a run samples.
    #[must_use]
    pub fn config(&self) -> RunConfig {
        self.track
            .as_ref()
            .map_or_else(|| self.common.config(Targets::Full), Track::config)
    }
}

/// Arguments of `lex verdict`.
#[derive(clap::Args, Debug)]
pub struct VerdictArgs {
    /// Snapshot directory of the base
    #[arg(required_unless_present = "readme")]
    pub base: Option<PathBuf>,
    /// Snapshot directory of the head
    #[arg(requires = "base")]
    pub head: Option<PathBuf>,
    /// Rewrite the Tracks table of the README from this snapshot directory
    #[arg(long, value_name = "SNAP_DIR", conflicts_with_all = ["base", "head"])]
    pub readme: Option<PathBuf>,
    /// With --readme, change nothing and fail when the README is stale
    #[arg(long, requires = "readme")]
    pub check: bool,
    /// README file that --readme edits
    #[arg(long, requires = "readme", default_value = "benchmarks/README.md")]
    pub file: PathBuf,
}

/// Arguments of `lex clear`.
#[derive(clap::Args, Debug)]
pub struct ClearArgs {
    /// Language whose corpus cache (and opening books) to delete
    #[arg(
        long,
        required_unless_present_any = ["profile", "books"],
        conflicts_with = "profile"
    )]
    pub lang: Option<Language>,
    /// Delete only the opening books, narrowed by --lang and -l (never touches corpora or Profiles)
    #[arg(long, conflicts_with = "profile")]
    pub books: bool,
    /// Profile whose fetched lists and opening books to delete (never touches corpora)
    #[arg(long)]
    pub profile: Option<Profile>,
    /// Delete only this word length (default: every length)
    #[arg(
        short = 'l',
        long,
        conflicts_with = "profile",
        value_parser = parse_word_length
    )]
    pub word_length: Option<usize>,
    /// Directory containing cached word-frequency files
    #[arg(long, default_value = "data")]
    pub data_dir: PathBuf,
}

/// Arguments shared by every subcommand.
#[derive(clap::Args, Debug)]
pub struct CommonArgs {
    /// Word length; the `help` below interpolates the supported range, which a doc comment cannot.
    #[arg(
        short = 'l',
        long,
        value_parser = parse_word_length,
        required_unless_present = "profile",
        conflicts_with = "profile",
        help = format!("Length of the words to use in the game ({MIN_WORD_LENGTH}–{MAX_WORD_LENGTH}); a profile fixes it")
    )]
    pub word_length: Option<usize>,
    /// Number of the words to allow in the guesser's dictionary (default: all words in the corpus)
    #[arg(long, conflicts_with = "profile")]
    pub dictionary_length: Option<usize>,
    /// Language corpus (english/eng, french/fre, german/ger, spanish/spa, italian/ita, russian/rus); a profile fixes it
    #[arg(long, required_unless_present = "profile", conflicts_with = "profile")]
    pub lang: Option<Language>,
    /// Play a profile: its answer list and guess pool, with its language and word length (wordle, wordle-all)
    #[arg(long)]
    pub profile: Option<Profile>,
    /// Directory containing cached word-frequency files
    #[arg(long, default_value = "data")]
    pub data_dir: PathBuf,
    /// Which words may be guessed: all (every word), hard (the official Wordle hard-mode rule: keep every green in place, reuse every green or yellow letter) or ultrahard (only words consistent with every feedback so far, stricter than hard)
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.pool
    )]
    pub pool: GuessPool,
    /// Answer prior temperature: higher flattens the frequency-based prior
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.temperature,
        value_parser = parse_temperature
    )]
    pub temperature: f64,
    /// Score bonus, in bits, per unit of probability that a guess is the answer
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.win_weight,
        value_parser = parse_win_weight
    )]
    pub win_weight: f64,
    /// Search the game tree exactly once this few candidates remain (0 disables)
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.endgame_threshold
    )]
    pub endgame_threshold: usize,
    /// Endgame risk aversion: minimize the mean of e^(aversion x guesses); 0 minimizes plain expected guesses
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.endgame_aversion,
        value_parser = parse_win_weight
    )]
    pub endgame_aversion: f64,
}

impl CommonArgs {
    /// The word length to monomorphize over: the profile's, else `-l` (clap requires one).
    ///
    /// # Panics
    ///
    /// If neither a profile nor `-l` is set, which clap rejects and only a hand-built value can
    /// reach.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "clap's required_unless_present makes -l mandatory without --profile, and a required argument is never absent"
    )]
    pub const fn dispatch_length(&self) -> usize {
        match self.profile {
            Some(profile) => profile.spec().word_length,
            None => self
                .word_length
                .expect("clap requires -l without --profile"),
        }
    }

    /// The game these arguments select.
    ///
    /// # Panics
    ///
    /// If neither a profile nor `--lang` is set, which clap rejects and only a hand-built value
    /// can reach.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "clap's required_unless_present makes --lang mandatory without --profile, and a required argument is never absent"
    )]
    pub const fn game(&self) -> Game {
        match self.profile {
            Some(profile) => Game::Profile(profile),
            None => Game::Corpus {
                language: self.lang.expect("clap requires --lang without --profile"),
                length: self.dispatch_length(),
                dictionary_length: self.dictionary_length,
            },
        }
    }

    /// The Strategy these arguments select.
    #[must_use]
    pub const fn strategy(&self) -> Strategy {
        Strategy {
            pool: self.pool,
            temperature: self.temperature,
            win_weight: self.win_weight,
            endgame_threshold: self.endgame_threshold,
            endgame_aversion: self.endgame_aversion,
        }
    }

    /// The run these arguments select, over `targets`.
    #[must_use]
    pub const fn config(&self, targets: Targets) -> RunConfig {
        RunConfig {
            game: self.game(),
            strategy: self.strategy(),
            targets,
        }
    }
}

/// Arguments of `lex simulate`.
#[derive(clap::Args, Debug)]
#[command(
    mut_arg("lang", |a| a.required_unless_present("track")),
    mut_arg("word_length", |a| a.required_unless_present("track"))
)]
pub struct SimulateArgs {
    /// The game and Strategy to play.
    #[command(flatten)]
    pub common: CommonArgs,
    /// Number of target words to sample (default: every word in the dictionary)
    #[arg(short = 'n', long, conflicts_with = "targets")]
    pub num_games: Option<usize>,
    /// Seed for sampling targets; the same seed and corpus give the same targets
    #[arg(long, default_value_t = 0, requires = "num_games")]
    pub seed: u64,
    /// File of target words (one per line) to play instead of sampling
    #[arg(long)]
    pub targets: Option<PathBuf>,
    /// Write the full JSON report here ('-' for stdout, which moves the summary to stderr)
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    /// Play fully online instead of building and using the opening book
    #[arg(long)]
    pub no_book: bool,
    /// Play a benchmark Track by id (its game, strategy and targets)
    #[arg(
        long,
        hide = true,
        value_parser = parse_track,
        conflicts_with_all = [
            "lang", "word_length", "dictionary_length", "profile", "pool", "temperature",
            "win_weight", "endgame_threshold", "endgame_aversion", "num_games", "seed", "targets",
        ]
    )]
    pub track: Option<Track>,
}

impl SimulateArgs {
    /// The length to monomorphize over.
    #[must_use]
    pub const fn dispatch_length(&self) -> usize {
        match self.track {
            Some(track) => track.game.word_length(),
            None => self.common.dispatch_length(),
        }
    }

    /// What these arguments ask to play.
    #[must_use]
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

/// Arguments of `lex compare`.
#[derive(clap::Args, Debug)]
pub struct CompareArgs {
    /// Baseline report
    pub baseline: PathBuf,
    /// Candidate report, played against the same targets
    pub candidate: PathBuf,
}

/// Arguments of `lex assist`.
#[derive(clap::Args, Debug)]
pub struct AssistArgs {
    /// The game and Strategy to play.
    #[command(flatten)]
    pub common: CommonArgs,
    /// Number of suggestions to show per turn
    #[arg(short = 's', long, default_value_t = 5)]
    pub suggestions: usize,
    /// Play fully online instead of building and using the opening book
    #[arg(long)]
    pub no_book: bool,
}

/// Parses a word length, rejecting one outside the supported range.
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

/// Parses a Track id, rejecting one that names no Track.
fn parse_track(s: &str) -> anyhow::Result<Track> {
    Track::find(s).ok_or_else(|| CliError::UnknownTrack(s.to_owned()).into())
}

/// Parses an Answer prior temperature, which must be positive and finite.
fn parse_temperature(s: &str) -> anyhow::Result<f64> {
    let t: f64 = s.parse()?;
    if t.is_finite() && t > 0.0 {
        Ok(t)
    } else {
        Err(CliError::InvalidTemperature(t).into())
    }
}

/// Parses a non-negative finite weight.
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
    fn books_stand_alone_or_narrow_by_language_and_length() {
        assert!(parse(&["--books"]).is_ok());
        assert!(parse(&["--books", "--lang", "english"]).is_ok());
        assert!(parse(&["--books", "--lang", "english", "-l", "5"]).is_ok());
        assert!(parse(&["--books", "-l", "5"]).is_ok());
        assert!(parse(&["--books", "--profile", "wordle"]).is_err());
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
        let Command::Assist(a) =
            Cli::try_parse_from(["lex", "assist"].into_iter().chain(args.iter().copied()))?.command
        else {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidSubcommand,
                "`assist` parsed to another command",
            ));
        };
        Ok(a.common)
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
        let Command::Simulate(a) =
            Cli::try_parse_from(["lex", "simulate"].into_iter().chain(args.iter().copied()))?
                .command
        else {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidSubcommand,
                "`simulate` parsed to another command",
            ));
        };
        Ok(a)
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

#[cfg(test)]
mod book_tests {
    use super::*;

    fn build(args: &[&str]) -> Result<BookBuildArgs, clap::Error> {
        let Command::Book(BookArgs {
            command: BookCommand::Build(a),
        }) = Cli::try_parse_from(
            ["lex", "book", "build"]
                .into_iter()
                .chain(args.iter().copied()),
        )?
        .command
        else {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidSubcommand,
                "`book build` parsed to another command",
            ));
        };
        Ok(a)
    }

    #[test]
    fn build_takes_simulates_game_and_strategy_flags() {
        assert!(build(&[]).is_err());
        let args = build(&["-l", "5", "--lang", "english", "--pool", "hard", "--force"]).unwrap();
        assert!(args.force);
        assert_eq!(args.config().strategy.pool, GuessPool::Hard);
        assert_eq!(args.config().targets, Targets::Full);
        assert_eq!(
            build(&["--profile", "wordle"]).unwrap().dispatch_length(),
            5
        );
    }

    #[test]
    fn build_takes_a_track_alone() {
        let args = build(&["--track", "english-5"]).unwrap();
        assert_eq!(args.config(), Track::find("english-5").unwrap().config());
        assert!(build(&["--track", "english-5", "--pool", "hard"]).is_err());
        assert!(build(&["--track", "english-5", "--force"]).is_ok());
    }

    #[test]
    fn simulate_and_assist_can_opt_out_of_the_book() {
        for command in ["simulate", "assist"] {
            let parse = |extra: &[&str]| {
                Cli::try_parse_from(
                    ["lex", command, "-l", "5", "--lang", "english"]
                        .into_iter()
                        .chain(extra.iter().copied()),
                )
            };
            assert!(parse(&["--no-book"]).is_ok(), "{command}");
        }
    }
}
