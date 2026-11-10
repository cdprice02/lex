//! The `clear` verb of the cache command groups (`dict`, `corpus`, `profile`, `book`). Each
//! deletes through `lex-data`, which returns what it removed, and prints every path and a total.

use std::path::PathBuf;

use lex_core::SOLVER_DIGEST;
use lex_data::{BookFilter, DataDir, Language, Profile};
use strum::VariantArray;

use crate::cli::{BookClearArgs, CorpusClearArgs, DictClearArgs, ProfileClearArgs};

/// What a `clear` removed, one path per line and a total.
#[must_use]
pub fn summary(removed: &[PathBuf]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for path in removed {
        let _ = writeln!(out, "removed {}", path.display());
    }
    let noun = if removed.len() == 1 { "path" } else { "paths" };
    let _ = write!(out, "{} {noun} removed", removed.len());
    out
}

/// Prints [`summary`].
fn report(removed: &[PathBuf]) {
    println!("{}", summary(removed));
}

/// The languages `--lang` names, or every language with `--all`.
fn languages(lang: Option<Language>) -> Vec<Language> {
    lang.map_or_else(|| Language::VARIANTS.to_vec(), |lang| vec![lang])
}

/// `lex dict clear`: the Dictionary, its language's Corpus caches at every length and their
/// books, after warning that the next fetch changes what filters the Corpus.
///
/// # Errors
///
/// If a cache file cannot be removed.
pub fn dict(args: &DictClearArgs) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    eprintln!(
        "warning: the next fetch takes KAIKKI's current (rolling) Wiktionary extract, which can \
         change the Corpus and the Track snapshots of the language"
    );
    let mut removed = Vec::new();
    for lang in languages(args.lang) {
        removed.extend(dir.clear_dict(lang)?);
    }
    report(&removed);
    Ok(())
}

/// `lex corpus clear`: Corpus caches with their books. A language with no cache removes nothing.
///
/// # Errors
///
/// If a cache file cannot be removed.
pub fn corpus(args: &CorpusClearArgs) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    let mut removed = Vec::new();
    if let Some(lang) = args.lang {
        removed.extend(dir.clear(lang, args.word_length)?);
    } else {
        for &lang in Language::VARIANTS {
            removed.extend(dir.clear(lang, None)?);
        }
    }
    report(&removed);
    Ok(())
}

/// `lex profile clear`: a Profile's lists with its books. A Profile that is not fetched removes
/// nothing.
///
/// # Errors
///
/// If a cache file cannot be removed.
pub fn profile(args: &ProfileClearArgs) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    let mut removed = Vec::new();
    if let Some(profile) = args.profile {
        removed.extend(dir.clear_profile(profile)?);
    } else {
        for &profile in Profile::VARIANTS {
            removed.extend(dir.clear_profile(profile)?);
        }
    }
    report(&removed);
    Ok(())
}

/// `lex book clear`: only the opening books, narrowed by `--lang` and `-l`; with `--stale`, only
/// the books `lex book list --stale` shows, narrowed also by `--profile`.
///
/// # Errors
///
/// If a books directory cannot be removed.
pub fn book(args: &BookClearArgs) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    if args.stale {
        let filter = BookFilter {
            lang: args.lang,
            word_length: args.word_length,
            profile: args.profile,
        };
        report(&dir.clear_stale_books(SOLVER_DIGEST, &filter)?);
        return Ok(());
    }
    report(&dir.clear_books(args.lang, args.word_length)?);
    Ok(())
}
