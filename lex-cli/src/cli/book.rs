//! The arguments of the `lex book` group.

use clap::Subcommand;
use lex_data::{Language, Profile};

use super::cache::DataDirArg;
use super::play::{CommonArgs, parse_track, parse_word_length};
use crate::config::{RunConfig, Targets};
use crate::tracks::Track;

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
