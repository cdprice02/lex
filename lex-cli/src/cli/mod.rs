//! The command line: clap definitions and their conversion to run configurations.
//!
//! `mod.rs` holds the root (`Cli`, `Command`, the thread pool); `play` the game and Strategy
//! arguments of `simulate`, `assist` and `compare`; `book` and `cache` the cache command groups.

mod book;
mod cache;
mod play;
#[cfg(test)]
mod tests;

use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use lex_data::Language;

use crate::error::CliError;
use crate::style::{ColorArg, SymbolScheme};

pub use book::{BookArgs, BookBuildArgs, BookClearArgs, BookCommand, BookListArgs, BookShowArgs};
pub use cache::{
    CorpusArgs, CorpusBuildArgs, CorpusClearArgs, CorpusCommand, CorpusListArgs, CorpusShowArgs,
    DataDirArg, DictArgs, DictClearArgs, DictCommand, DictFetchArgs, DictListArgs, DictShowArgs,
    ProfileArgs, ProfileClearArgs, ProfileCommand, ProfileFetchArgs, ProfileListArgs,
    ProfileShowArgs,
};
pub use play::{AssistArgs, CommonArgs, CompareArgs, SimulateArgs};

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
    /// Parallel shard downloads; default measured, or --threads when given
    #[arg(long, value_name = "N", global = true)]
    pub download_threads: Option<NonZeroUsize>,
    /// Make no network request: use cached data, build what can be computed locally, and fail
    /// naming the fetch command for anything missing
    #[arg(long, global = true)]
    pub offline: bool,
    /// Whether `--threads` was given on the command line rather than defaulted, set by
    /// [`Cli::parse_args`].
    #[arg(skip)]
    pub threads_explicit: bool,
}

impl Cli {
    /// Parses `args` (the program name first) like [`Parser::try_parse_from`], and records
    /// whether `--threads` was given, which the derive alone cannot tell from its default.
    ///
    /// # Errors
    ///
    /// As [`Parser::try_parse_from`].
    pub fn parse_args<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let matches = Self::command().try_get_matches_from(args)?;
        let mut cli = Self::from_arg_matches(&matches)?;
        cli.threads_explicit = matches.value_source("threads") == Some(ValueSource::CommandLine);
        Ok(cli)
    }

    /// The shard download count the user asked for: `--download-threads`, else an explicit
    /// `--threads`, else `None` (the default applies). lex-data caps it at the shard count.
    #[must_use]
    pub fn download_request(&self) -> Option<NonZeroUsize> {
        self.download_threads
            .or_else(|| self.threads_explicit.then_some(self.threads))
    }
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
