//! Building the Corpus: from cached Ngrams totals when they are complete, else from a download.

use std::collections::{HashMap, HashSet};

use anyhow::Context;

use super::MIN_FREQUENCY;
use super::write::write_bucket;
use crate::data_dir::DataDir;
use crate::language::Language;
use crate::ngrams;
use crate::totals;
use crate::wiktionary;

/// Builds the ngrams cache for `lang` at word length `n` if it does not already exist.
/// Ensures the Wiktionary dict is present first, then downloads all ngrams shards keeping only
/// Wiktionary-valid words of supported lengths, and writes every length bucket to disk.
pub fn build_if_missing(dir: &DataDir, lang: Language, n: usize) -> anyhow::Result<()> {
    if !dir.ngrams_path(lang, n).exists() {
        build_all(dir, lang, &|_, _| {})?;
    }
    Ok(())
}

/// Builds the Corpus of `lang` at every length, fetching the Dictionary first when it is absent.
///
/// With complete Ngrams totals cached the Corpus is filtered from them with no network, and only
/// the lengths not already cached are written, so an existing Corpus file is never touched.
/// Otherwise every shard is downloaded once, the totals are written, and the Corpus is filtered
/// from the same totals and written at every length. `progress(done, total)` reports finished
/// shards of a download.
pub fn build_all(
    dir: &DataDir,
    lang: Language,
    progress: &(dyn Fn(u32, u32) + Sync),
) -> anyhow::Result<()> {
    build_all_via(dir, lang, progress, &|progress| {
        ngrams::fetch(lang, dir.download_workers(), progress)
    })
}

/// The download of a language's Ngrams totals, taking the shard progress callback.
pub type Download<'a> = dyn Fn(&(dyn Fn(u32, u32) + Sync)) -> anyhow::Result<totals::ByLength> + 'a;

/// [`build_all`] with the download injected: `download` returns the Ngrams totals of `lang`.
pub fn build_all_via(
    dir: &DataDir,
    lang: Language,
    progress: &(dyn Fn(u32, u32) + Sync),
    download: &Download<'_>,
) -> anyhow::Result<()> {
    if !dir.dict_path(lang).exists() {
        dir.fetch_dict(lang)?;
    }
    let valid_words = wiktionary::load_valid_words(lang, dir.as_ref())?;
    match totals::inspect(dir, lang) {
        totals::Inspection::Complete(meta) => {
            match totals::read_filtered(dir, lang, &meta, &valid_words) {
                Ok(by_length) => {
                    log::warn!("Building {lang} corpus from the cached Ngrams totals...");
                    populate_missing(dir, lang, &by_length)?;
                    log::info!("Cached {lang} corpus ({} length buckets)", by_length.len());
                    return Ok(());
                }
                Err(e) => log::warn!(
                    "the cached {lang} Ngrams totals are unusable ({e:#}); downloading them again"
                ),
            }
        }
        totals::Inspection::Incomplete(reason) => log::warn!(
            "the cached {lang} Ngrams totals are incomplete ({reason}); downloading them again"
        ),
        totals::Inspection::Absent => {}
    }
    dir.require_online(
        &format!("the complete {lang} Ngrams totals"),
        &format!("lex corpus build --lang {lang}"),
    )?;
    log::warn!("Building {lang} corpus...");
    let mut by_length = download(progress)?;
    totals::write(dir, lang, &by_length)?;
    filter_by_dictionary(&mut by_length, &valid_words);
    let n_buckets = by_length.len();
    populate(dir, lang, &by_length)?;
    log::info!("Cached {lang} corpus ({n_buckets} length buckets)");
    Ok(())
}

/// Keeps the words in `valid` and drops the lengths left empty, as the Corpus holds them.
fn filter_by_dictionary(by_length: &mut totals::ByLength, valid: &HashSet<String>) {
    for words in by_length.values_mut() {
        words.retain(|word, &mut count| count >= MIN_FREQUENCY && valid.contains(word));
    }
    by_length.retain(|_, words| !words.is_empty());
}

/// Writes one Cache per length bucket of `by_length`.
pub fn populate(
    dir: &DataDir,
    lang: Language,
    by_length: &HashMap<usize, HashMap<String, u64>>,
) -> anyhow::Result<()> {
    for (&n, words) in by_length {
        write_bucket(dir, lang, n, words)
            .with_context(|| format!("writing ngrams for {lang} length {n}"))?;
    }
    Ok(())
}

/// Writes the Cache of each length of `by_length` that is not on disk, leaving the others as
/// they are.
fn populate_missing(
    dir: &DataDir,
    lang: Language,
    by_length: &totals::ByLength,
) -> anyhow::Result<()> {
    for (&n, words) in by_length {
        if !dir.ngrams_path(lang, n).exists() {
            write_bucket(dir, lang, n, words)
                .with_context(|| format!("writing ngrams for {lang} length {n}"))?;
        }
    }
    Ok(())
}
