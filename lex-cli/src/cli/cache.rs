//! The arguments of the `lex dict`, `lex corpus` and `lex profile` groups, and the data directory
//! option every cache command shares.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::Subcommand;
use lex_data::{Language, Profile};
use strum::VariantArray as _;

use super::play::parse_word_length;

/// The data directory option every cache command shares.
#[derive(clap::Args, Debug)]
pub struct DataDirArg {
    /// Directory containing cached word-frequency files
    #[arg(long, default_value = "data")]
    pub data_dir: PathBuf,
}

/// The languages `--lang` names, or every language with `--all`.
#[must_use]
pub fn languages(lang: Option<Language>) -> Vec<Language> {
    lang.map_or_else(|| Language::VARIANTS.to_vec(), |lang| vec![lang])
}

impl DataDirArg {
    /// The data directory.
    #[must_use]
    pub fn dir(&self) -> lex_data::DataDir {
        lex_data::DataDir::new(&self.data_dir)
    }
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
    #[arg(long, conflicts_with = "offline")]
    pub force: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
    /// Whether downloads are forbidden, copied from [`super::Cli::offline`] by `main`.
    #[arg(skip)]
    pub offline: bool,
}

impl DictFetchArgs {
    /// The data directory, offline when the user asked for that.
    #[must_use]
    pub fn dir(&self) -> lex_data::DataDir {
        self.data.dir().offline(self.offline)
    }
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
    /// Build again even when cached: clears the Corpus caches and their opening books, then rebuilds from the cached Dictionary, and from the cached Ngrams totals when there are any (no download)
    #[arg(long, conflicts_with = "offline")]
    pub force: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
    /// Shard downloads at once, copied from [`super::Cli::download_request`] by `main`.
    #[arg(skip)]
    pub download_workers: Option<NonZeroUsize>,
    /// Whether downloads are forbidden, copied from [`super::Cli::offline`] by `main`.
    #[arg(skip)]
    pub offline: bool,
}

impl CorpusBuildArgs {
    /// The data directory, set to download as many shards at once as the user asked for.
    #[must_use]
    pub fn dir(&self) -> lex_data::DataDir {
        self.data
            .dir()
            .with_download_workers(self.download_workers)
            .offline(self.offline)
    }
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
    /// Also delete the Ngrams totals, which a plain clear keeps so the Corpus rebuilds with no network; the next build downloads every shard again
    #[arg(long, conflicts_with = "word_length")]
    pub all_data: bool,
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
    #[arg(long, conflicts_with = "offline")]
    pub force: bool,
    /// Where the cache lives.
    #[command(flatten)]
    pub data: DataDirArg,
    /// Shard downloads at once, copied from [`super::Cli::download_request`] by `main`.
    #[arg(skip)]
    pub download_workers: Option<NonZeroUsize>,
    /// Whether downloads are forbidden, copied from [`super::Cli::offline`] by `main`.
    #[arg(skip)]
    pub offline: bool,
}

impl ProfileFetchArgs {
    /// The data directory, set to download as many shards at once as the user asked for.
    #[must_use]
    pub fn dir(&self) -> lex_data::DataDir {
        self.data
            .dir()
            .with_download_workers(self.download_workers)
            .offline(self.offline)
    }
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
