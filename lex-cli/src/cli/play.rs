//! The arguments that choose and play a game: [`CommonArgs`] and the commands built on it.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use lex_core::{GuessPool, PriorShape, Strategy};
use lex_data::{Language, MAX_WORD_LENGTH, MIN_WORD_LENGTH, Profile};

use crate::config::{Game, RunConfig, Targets};
use crate::error::CliError;
use crate::style::SymbolScheme;
use crate::tracks::Track;

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
    /// Compare this many top-scored guesses by playing each out at mid-sized candidate sets (0 disables)
    #[arg(
        long,
        default_value_t = Strategy::DEFAULT.rollout_k
    )]
    pub rollout_k: usize,
    /// How frequency becomes the Answer prior weight: power, sigmoid:<cutoff in (0, 1]> or floor:<percentile in [0, 100)>
    #[arg(
        long,
        hide = true,
        default_value_t = Strategy::DEFAULT.prior_shape
    )]
    pub prior_shape: PriorShape,
    /// Shard downloads at once, copied from [`Cli::download_request`] by `main`.
    #[arg(skip)]
    pub download_workers: Option<NonZeroUsize>,
    /// Whether downloads are forbidden, copied from [`Cli::offline`] by `main`.
    #[arg(skip)]
    pub offline: bool,
}

impl CommonArgs {
    /// The data directory, set to download as many shards at once as the user asked for.
    #[must_use]
    pub fn data(&self) -> lex_data::DataDir {
        lex_data::DataDir::new(&self.data_dir)
            .with_download_workers(self.download_workers)
            .offline(self.offline)
    }

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
            rollout_k: self.rollout_k,
            prior_shape: self.prior_shape,
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
            "win_weight", "endgame_threshold", "endgame_aversion", "rollout_k", "prior_shape", "num_games", "seed",
            "targets",
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
    /// Show only the suggested guess, without the scores or the grading line
    #[arg(long)]
    pub no_explain: bool,
    /// Show the full numbers: bits, every Guess cost, the opening book, realised bits
    #[arg(long, conflicts_with = "no_explain")]
    pub details: bool,
    /// The display scheme for echoed feedback, copied from the global `--symbols` by `main`.
    #[arg(skip)]
    pub symbols: SymbolScheme,
}

/// Parses a word length, rejecting one outside the supported range.
pub fn parse_word_length(s: &str) -> anyhow::Result<usize> {
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
pub fn parse_track(s: &str) -> anyhow::Result<Track> {
    Track::find(s).ok_or_else(|| CliError::UnknownTrack(s.to_owned()).into())
}

/// Parses an Answer prior temperature, which must be positive and finite.
pub fn parse_temperature(s: &str) -> anyhow::Result<f64> {
    let t: f64 = s.parse()?;
    if t.is_finite() && t > 0.0 {
        Ok(t)
    } else {
        Err(CliError::InvalidTemperature(t).into())
    }
}

/// Parses a non-negative finite weight.
pub fn parse_win_weight(s: &str) -> anyhow::Result<f64> {
    let w: f64 = s.parse()?;
    if w.is_finite() && w >= 0.0 {
        Ok(w)
    } else {
        Err(CliError::InvalidWinWeight(w).into())
    }
}
