//! The command line: clap definitions and their conversion to run configurations.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lex_core::{GuessPool, PriorShape, Strategy};
use lex_data::{Language, Profile};

use crate::config::{Game, RunConfig, Targets};
use crate::error::CliError;
use crate::style::{ColorArg, SymbolScheme};
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
    /// When to colour output
    #[arg(long, value_enum, default_value_t, global = true)]
    pub color: ColorArg,
    /// How feedback tiles are displayed (input accepts every scheme regardless)
    #[arg(long, value_enum, default_value_t, global = true)]
    pub symbols: SymbolScheme,
    /// Show no progress bars, spinners or progress lines
    #[arg(long, global = true)]
    pub no_progress: bool,
    /// Worker threads (at least 1); defaults to the available parallelism
    #[arg(long, value_name = "N", default_value_t = default_threads(), global = true)]
    pub threads: NonZeroUsize,
}

/// The thread count when `--threads` is absent: the available parallelism, else 1.
#[must_use]
pub fn default_threads() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}

/// Builds the one rayon pool of a run, `threads` wide. The command runs inside `install`, so the
/// ambient pool carries the thread budget and nothing else sees the count.
///
/// # Errors
///
/// [`CliError::ThreadPool`] when the operating system refuses the threads.
pub fn build_pool(threads: NonZeroUsize) -> Result<rayon::ThreadPool, CliError> {
    Ok(rayon::ThreadPoolBuilder::new()
        .num_threads(threads.get())
        .build()?)
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
    /// Manage the cached Wiktionary dictionaries
    #[command(alias = "dictionary")]
    Dict(DictArgs),
    /// Manage the cached word-frequency corpora
    Corpus(CorpusArgs),
    /// Manage the fetched Profile lists
    Profile(ProfileArgs),
    /// Manage opening book files
    Book(BookArgs),
    /// Judge a head set of Track snapshots against a base set, or refresh the README table
    #[command(hide = true)]
    Verdict(VerdictArgs),
    /// Look words up: Dictionary membership, Corpus frequency and rank, Prior, Profile lists (never downloads)
    Word(WordArgs),
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
    /// List the stored opening books with their status
    List(BookListArgs),
    /// Show one stored opening book, by id or by the game and Strategy that would read it
    Show(Box<BookShowArgs>),
    /// Delete opening books so they are rebuilt on next use
    Clear(BookClearArgs),
    /// Build the opening book for a game and strategy, or report that it is up to date
    Build(Box<BookBuildArgs>),
}

/// Arguments of `lex book list`.
#[derive(clap::Args, Debug)]
pub struct BookListArgs {
    /// List only this language's books
    #[arg(long)]
    pub lang: Option<Language>,
    /// List only books of this word length
    #[arg(short = 'l', long, value_parser = parse_word_length)]
    pub word_length: Option<usize>,
    /// List only books built for this Profile
    #[arg(long)]
    pub profile: Option<Profile>,
    /// List only the books `lex book clear --stale` would delete (stale or corrupt)
    #[arg(long)]
    pub stale: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex book show`: an id from `list`, or the game and Strategy of `book build`.
#[derive(clap::Args, Debug)]
#[command(
    mut_arg("lang", |a| a.required_unless_present("id")),
    mut_arg("word_length", |a| a.required_unless_present("id"))
)]
pub struct BookShowArgs {
    /// Id (or a unique prefix) from `lex book list`
    #[arg(conflicts_with_all = [
        "lang", "word_length", "dictionary_length", "profile", "pool", "temperature",
        "win_weight", "endgame_threshold", "endgame_aversion", "rollout_k", "prior_shape",
    ])]
    pub id: Option<String>,
    /// The game and Strategy whose book to show.
    #[command(flatten)]
    pub common: CommonArgs,
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
            "win_weight", "endgame_threshold", "endgame_aversion", "rollout_k",
            "prior_shape",
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

/// The data directory option every cache command shares.
#[derive(clap::Args, Debug)]
pub struct DataDirArg {
    /// Directory containing cached word-frequency files
    #[arg(long, default_value = "data")]
    pub data_dir: PathBuf,
}

/// Arguments of `lex word`.
#[derive(clap::Args, Debug)]
pub struct WordArgs {
    /// Words to look up; each is normalized as a Corpus build does (NFC, lowercase)
    #[arg(required = true, value_name = "WORD")]
    pub words: Vec<String>,
    /// Language whose Dictionary, Corpus and Profiles to look in
    #[arg(long)]
    pub lang: Language,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex dict`.
#[derive(clap::Args, Debug)]
pub struct DictArgs {
    /// The dictionary operation.
    #[command(subcommand)]
    pub command: DictCommand,
}

/// The operations of `lex dict`.
#[derive(Subcommand, Debug)]
pub enum DictCommand {
    /// List the cached Dictionaries (never downloads)
    List(DictListArgs),
    /// Show one cached Dictionary in full (never downloads)
    Show(DictShowArgs),
    /// Download a Dictionary ahead of time (a no-op when cached; --force takes KAIKKI's current extract)
    Fetch(DictFetchArgs),
    /// Delete a Dictionary, the Corpus caches of its language at every length and their opening books
    Clear(DictClearArgs),
}

/// Arguments of `lex dict list`.
#[derive(clap::Args, Debug)]
pub struct DictListArgs {
    /// List only this language
    #[arg(long)]
    pub lang: Option<Language>,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex dict show`.
#[derive(clap::Args, Debug)]
pub struct DictShowArgs {
    /// Language whose Dictionary to show
    #[arg(long)]
    pub lang: Language,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex dict fetch`. One Dictionary serves every word length, so there is no `-l`.
#[derive(clap::Args, Debug)]
pub struct DictFetchArgs {
    /// Language whose Dictionary to fetch
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    pub lang: Option<Language>,
    /// Fetch the Dictionary of every language
    #[arg(long)]
    pub all: bool,
    /// Fetch again even when cached: clears the Dictionary, the Corpus caches built from it and their opening books first, then takes KAIKKI's current extract, which can change the Corpus
    #[arg(long)]
    pub force: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex dict clear`. One Dictionary serves every word length, so there is no `-l`.
#[derive(clap::Args, Debug)]
pub struct DictClearArgs {
    /// Language whose Dictionary (with its corpora and opening books) to delete; the next fetch takes KAIKKI's current extract, which can change the Corpus
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    pub lang: Option<Language>,
    /// Delete the Dictionary of every language
    #[arg(long)]
    pub all: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex corpus`.
#[derive(clap::Args, Debug)]
pub struct CorpusArgs {
    /// The corpus operation.
    #[command(subcommand)]
    pub command: CorpusCommand,
}

/// The operations of `lex corpus`.
#[derive(Subcommand, Debug)]
pub enum CorpusCommand {
    /// List the cached Corpora from their headers (never downloads)
    List(CorpusListArgs),
    /// Show one cached Corpus in full, verifying its checksum (never downloads)
    Show(CorpusShowArgs),
    /// Build the Corpus of a language at every length ahead of time (a no-op when cached; --force rebuilds)
    Build(CorpusBuildArgs),
    /// Delete a cached Corpus and its opening books so they are rebuilt on next use (the Dictionary is kept)
    Clear(CorpusClearArgs),
}

/// Arguments of `lex corpus list`.
#[derive(clap::Args, Debug)]
pub struct CorpusListArgs {
    /// List only this language
    #[arg(long)]
    pub lang: Option<Language>,
    /// List only this word length
    #[arg(short = 'l', long, value_parser = parse_word_length)]
    pub word_length: Option<usize>,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex corpus show`.
#[derive(clap::Args, Debug)]
pub struct CorpusShowArgs {
    /// Language of the Corpus to show
    #[arg(long)]
    pub lang: Language,
    /// Word length of the Corpus to show
    #[arg(short = 'l', long, value_parser = parse_word_length)]
    pub word_length: usize,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex corpus build`. A build covers every word length, so there is no `-l`.
#[derive(clap::Args, Debug)]
pub struct CorpusBuildArgs {
    /// Language whose Corpus to build (its Dictionary is fetched first if absent)
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    pub lang: Option<Language>,
    /// Build the Corpus of every language
    #[arg(long)]
    pub all: bool,
    /// Build again even when cached: clears the Corpus caches and their opening books, then rebuilds from the cached Dictionary
    #[arg(long)]
    pub force: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex corpus clear`.
#[derive(clap::Args, Debug)]
pub struct CorpusClearArgs {
    /// Language whose Corpus cache (and opening books) to delete
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    pub lang: Option<Language>,
    /// Delete only this word length (default: every length)
    #[arg(
        short = 'l',
        long,
        requires = "lang",
        conflicts_with = "all",
        value_parser = parse_word_length
    )]
    pub word_length: Option<usize>,
    /// Delete the Corpus caches of every language
    #[arg(long)]
    pub all: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex profile`.
#[derive(clap::Args, Debug)]
pub struct ProfileArgs {
    /// The profile operation.
    #[command(subcommand)]
    pub command: ProfileCommand,
}

/// The operations of `lex profile`.
#[derive(Subcommand, Debug)]
pub enum ProfileCommand {
    /// List the fetched Profiles, verifying their lists (never downloads)
    List(ProfileListArgs),
    /// Show one fetched Profile's lists and checksums (never downloads)
    Show(ProfileShowArgs),
    /// Fetch a Profile's lists ahead of time, building its Corpus if absent (a no-op when fetched; --force refetches)
    Fetch(ProfileFetchArgs),
    /// Delete a Profile's fetched lists and opening books so they are fetched again on next use (corpora are kept)
    Clear(ProfileClearArgs),
}

/// Arguments of `lex profile list`.
#[derive(clap::Args, Debug)]
pub struct ProfileListArgs {
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex profile show`.
#[derive(clap::Args, Debug)]
pub struct ProfileShowArgs {
    /// Profile to show
    #[arg(long)]
    pub profile: Profile,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex profile fetch`.
#[derive(clap::Args, Debug)]
pub struct ProfileFetchArgs {
    /// Profile whose lists to fetch (its language's Corpus is built first if absent)
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    pub profile: Option<Profile>,
    /// Fetch every Profile
    #[arg(long)]
    pub all: bool,
    /// Fetch again even when fetched: clears the Profile's lists and opening books first, then re-verifies the checksums
    #[arg(long)]
    pub force: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex profile clear`.
#[derive(clap::Args, Debug)]
pub struct ProfileClearArgs {
    /// Profile whose fetched lists and opening books to delete
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    pub profile: Option<Profile>,
    /// Delete every Profile
    #[arg(long)]
    pub all: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
}

/// Arguments of `lex book clear`.
#[derive(clap::Args, Debug)]
pub struct BookClearArgs {
    /// Language whose opening books to delete
    #[arg(long, required_unless_present_any = ["all", "stale"], conflicts_with = "all")]
    pub lang: Option<Language>,
    /// Delete only this word length (default: every length)
    #[arg(
        short = 'l',
        long,
        requires = "lang",
        conflicts_with = "all",
        value_parser = parse_word_length
    )]
    pub word_length: Option<usize>,
    /// Delete only the books `lex book list --stale` shows (stale or corrupt), narrowed by --lang, -l and --profile
    #[arg(long, conflicts_with = "all")]
    pub stale: bool,
    /// With --stale, only books built for this Profile
    #[arg(long, requires = "stale")]
    pub profile: Option<Profile>,
    /// Delete the opening books of every language
    #[arg(long)]
    pub all: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
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
    /// The display scheme for echoed feedback, copied from the global `--symbols` by `main`.
    #[arg(skip)]
    pub symbols: SymbolScheme,
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
    fn display_flags_are_global_and_default_to_auto_letters() {
        let base = ["lex", "assist", "--lang", "english", "-l", "5"];
        let cli = Cli::try_parse_from(base).unwrap();
        assert_eq!(
            (cli.color, cli.symbols),
            (ColorArg::Auto, SymbolScheme::Letters)
        );
        let cli = Cli::try_parse_from(base.into_iter().chain([
            "--color",
            "always",
            "--symbols",
            "emoji",
        ]))
        .unwrap();
        assert_eq!(
            (cli.color, cli.symbols),
            (ColorArg::Always, SymbolScheme::Emoji)
        );
        let cli = Cli::try_parse_from([
            "lex",
            "--color",
            "never",
            "--symbols",
            "digits",
            "book",
            "clear",
            "--all",
        ])
        .unwrap();
        assert_eq!(
            (cli.color, cli.symbols),
            (ColorArg::Never, SymbolScheme::Digits)
        );
        assert!(Cli::try_parse_from(["lex", "--symbols", "c", "book", "clear", "--all"]).is_err());
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
    fn the_prior_shape_defaults_to_power_and_rejects_bad_values() {
        let base = ["--lang", "english", "-l", "5"];
        let shape = |extra: &[&str]| {
            let args: Vec<&str> = base.iter().chain(extra).copied().collect();
            simulate(&args).map(|a| a.common.strategy().prior_shape)
        };
        assert_eq!(shape(&[]).unwrap(), PriorShape::Power);
        assert_eq!(
            shape(&["--prior-shape", "sigmoid:0.25"]).unwrap(),
            PriorShape::Sigmoid { cutoff: 0.25 }
        );
        assert_eq!(
            shape(&["--prior-shape", "floor:50"]).unwrap(),
            PriorShape::Floor { percentile: 50.0 }
        );
        for bad in ["sigmoid:0", "sigmoid:2", "floor:100", "floor:-5", "bogus"] {
            assert!(shape(&["--prior-shape", bad]).is_err(), "{bad}");
        }
        assert!(simulate(&["--track", "english-5", "--prior-shape", "power"]).is_err());
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
        Ok(*a)
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

#[cfg(test)]
mod threads_tests {
    use super::*;

    const ASSIST: [&str; 6] = ["assist", "--lang", "english", "-l", "5", "--no-progress"];

    fn threads(args: &[&str]) -> Result<NonZeroUsize, clap::Error> {
        Cli::try_parse_from(std::iter::once("lex").chain(args.iter().copied())).map(|c| c.threads)
    }

    #[test]
    fn threads_accepts_one_and_more() {
        assert_eq!(
            threads(&["--threads", "1", "book", "clear", "--all"])
                .unwrap()
                .get(),
            1
        );
        assert_eq!(
            threads(&["--threads", "7", "book", "clear", "--all"])
                .unwrap()
                .get(),
            7
        );
    }

    #[test]
    fn threads_rejects_zero_and_non_numbers() {
        assert!(threads(&["--threads", "0", "book", "clear", "--all"]).is_err());
        assert!(threads(&["--threads", "-1", "book", "clear", "--all"]).is_err());
        assert!(threads(&["--threads", "many", "book", "clear", "--all"]).is_err());
    }

    #[test]
    fn threads_defaults_to_the_available_parallelism() {
        let default = threads(&ASSIST).unwrap();
        assert!(default.get() >= 1);
        assert_eq!(default, default_threads());
    }

    #[test]
    fn threads_is_global_before_and_after_the_subcommand() {
        let after: Vec<&str> = ASSIST.into_iter().chain(["--threads", "2"]).collect();
        assert_eq!(threads(&after).unwrap().get(), 2);
        let before: Vec<&str> = ["--threads", "3"].into_iter().chain(ASSIST).collect();
        assert_eq!(threads(&before).unwrap().get(), 3);
        assert_eq!(
            threads(&["book", "clear", "--all", "--threads", "4"])
                .unwrap()
                .get(),
            4
        );
    }

    #[test]
    fn the_pool_has_the_requested_width() {
        let pool = build_pool(NonZeroUsize::new(3).unwrap()).unwrap();
        assert_eq!(pool.install(rayon::current_num_threads), 3);
    }
}
