//! Downloading and filtering the Google Books Ngrams V3 shards into word counts.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use anyhow::Context;
use flate2::read::MultiGzDecoder;

use crate::http;
use crate::language::Language;
use crate::parse::parse_ngram_line;
use crate::store::MIN_FREQUENCY;
use crate::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

/// Download workers when the user asked for no count: from 4 up, a build runs at the speed of the
/// link, and more workers only add CPU time and memory
/// (`benchmarks/experiments/download-concurrency.md`).
pub const DEFAULT_DOWNLOAD_WORKERS: usize = 4;

/// Download workers for a language of `shards` shards: the count the user asked for, else
/// [`DEFAULT_DOWNLOAD_WORKERS`], capped by the shard count.
fn worker_count(requested: Option<NonZeroUsize>, shards: u32) -> usize {
    requested
        .map_or(DEFAULT_DOWNLOAD_WORKERS, NonZeroUsize::get)
        .min(usize::try_from(shards).unwrap_or(usize::MAX))
}

/// Google Books Ngrams V3 snapshot the corpus is built from. The snapshot is frozen, but
/// recording it keeps run provenance complete.
pub const SNAPSHOT: &str = "20200217";

/// Downloads all Google Books Ngrams V3 shards for `lang` with up to `requested`
/// requests in flight at once (see [`worker_count`]). Returns the
/// Ngrams totals: every word that passes `is_valid_word` at a supported length with its count
/// summed over all years, grouped by char count, before any Dictionary filter.
///
/// Words are reduced as each shard streams in and each worker folds its shards into one map, so
/// memory holds the distinct valid words rather than every token of the corpus.
///
/// `progress(done, total)` is called once with 0 before the first request and after each shard
/// finishes, from the worker threads.
///
/// Workers pull shard indices from a shared counter; a shard that fails after its retries stops
/// the others from starting new ones. Counts are summed, so the result does not depend on the
/// order shards finish in.
pub fn fetch(
    lang: Language,
    requested: Option<NonZeroUsize>,
    progress: &(dyn Fn(u32, u32) + Sync),
) -> anyhow::Result<HashMap<usize, HashMap<String, u64>>> {
    let n = lang.shard_count();
    progress(0, n);
    let finished = AtomicU32::new(0);
    let agent = http::agent();
    let next = AtomicU32::new(0);
    let failed = AtomicBool::new(false);

    let worker = || -> anyhow::Result<HashMap<String, u64>> {
        let mut folded: HashMap<String, u64> = HashMap::new();
        while !failed.load(Ordering::Relaxed) {
            let shard = next.fetch_add(1, Ordering::Relaxed);
            if shard >= n {
                break;
            }
            let url = format!(
                "https://storage.googleapis.com/books/ngrams/books/{SNAPSHOT}/{}/1-{shard:05}-of-{n:05}.gz",
                lang.lang_code()
            );
            log::debug!("  [{}/{}] {url}", shard + 1, n);
            match http::get(&agent, &url, |body| {
                read_shard(BufReader::new(MultiGzDecoder::new(body)))
            }) {
                Ok(words) => {
                    log::info!("  [{}/{}] done", shard + 1, n);
                    progress(finished.fetch_add(1, Ordering::Relaxed) + 1, n);
                    fold(&mut folded, words);
                }
                Err(e) => {
                    failed.store(true, Ordering::Relaxed);
                    return Err(e.context(format!("fetching shard {shard} for {lang}")));
                }
            }
        }
        Ok(folded)
    };

    let results: Vec<_> = thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..worker_count(requested, n) {
            workers.push(scope.spawn(worker));
        }
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });

    let folded = results
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()
        .with_context(|| format!("fetching ngrams for {lang}"))?;
    Ok(by_length(folded))
}

/// Adds every count of `from` into `into`.
fn fold(into: &mut HashMap<String, u64>, from: HashMap<String, u64>) {
    for (word, count) in from {
        *into.entry(word).or_insert(0) += count;
    }
}

/// Sums the per-worker maps and groups the words by char count. A word's total, not a single
/// shard's share of it, is what is kept, so the result does not depend on how shards were
/// divided among workers. Words with no count are dropped.
fn by_length(
    folded: impl IntoIterator<Item = HashMap<String, u64>>,
) -> HashMap<usize, HashMap<String, u64>> {
    let mut maps = folded.into_iter();
    let mut total = maps.next().unwrap_or_default();
    for map in maps {
        fold(&mut total, map);
    }
    let mut by_length: HashMap<usize, HashMap<String, u64>> = HashMap::new();
    for (word, count) in total {
        if count >= MIN_FREQUENCY {
            by_length
                .entry(word.chars().count())
                .or_default()
                .insert(word, count);
        }
    }
    by_length
}

/// Sums the counts of every valid word whose length is supported, in one decompressed shard.
/// A normalized word can span shards, so these are partial counts until merged.
fn read_shard(reader: impl BufRead) -> std::io::Result<HashMap<String, u64>> {
    let mut acc: HashMap<String, u64> = HashMap::new();
    for line in reader.lines() {
        if let Ok((word, count)) = parse_ngram_line(&line?) {
            let len = word.chars().count();
            if (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&len) {
                *acc.entry(word).or_insert(0) += count;
            }
        }
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workers(requested: Option<usize>, shards: u32) -> usize {
        worker_count(requested.and_then(NonZeroUsize::new), shards)
    }

    #[test]
    fn worker_count_honors_the_requested_count() {
        assert_eq!(workers(Some(1), 24), 1);
        assert_eq!(workers(Some(12), 24), 12);
        assert_eq!(workers(Some(24), 24), 24);
    }

    #[test]
    fn worker_count_defaults_when_nothing_is_requested() {
        assert_eq!(workers(None, 24), DEFAULT_DOWNLOAD_WORKERS);
    }

    #[test]
    fn worker_count_is_capped_by_the_shard_count() {
        assert_eq!(workers(Some(16), 2), 2);
        assert_eq!(workers(None, 2), 2);
    }

    #[test]
    fn read_shard_sums_repeated_words_and_skips_invalid_lines() {
        let shard = "crane\t2000,5,1\t2001,7,1\n\
                     crane\t1999,3,1\n\
                     stare_NOUN\t2000,9,1\n\
                     two words\t2000,9,1\n\
                     no tab here\n\
                     light\t2000,4,1\n";
        let words = read_shard(shard.as_bytes()).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words["crane"], 15);
        assert_eq!(words["light"], 4);
    }

    #[test]
    fn read_shard_keeps_dictionary_blind_words_and_drops_unsupported_lengths() {
        let shard = "crane\t2000,5,1\n\
                     swipe\t2000,5,1\n\
                     ab\t2000,5,1\n\
                     abcdefghijklmnopqrstuv\t2000,5,1\n";
        let words = read_shard(shard.as_bytes()).unwrap();
        let mut kept: Vec<_> = words.keys().map(String::as_str).collect();
        kept.sort_unstable();
        assert_eq!(kept, ["crane", "swipe"]);
    }

    #[test]
    fn by_length_sums_a_word_across_workers_and_groups_by_length() {
        let worker = |pairs: &[(&str, u64)]| -> HashMap<String, u64> {
            pairs.iter().map(|&(w, c)| (w.to_owned(), c)).collect()
        };
        let totals = by_length([
            worker(&[("crane", 5), ("ace", 1)]),
            worker(&[("crane", 7), ("zero", 0)]),
        ]);
        assert_eq!(totals[&5]["crane"], 12);
        assert_eq!(totals[&3]["ace"], 1);
        assert!(!totals.contains_key(&4), "a zero total is dropped");
        assert!(by_length([]).is_empty());
    }

    #[test]
    fn gzipped_fixture_decodes_through_the_shard_reader() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"crane\t2000,5,1\nlight\t2000,4,1\n")
            .unwrap();
        let gz = encoder.finish().unwrap();
        let words = read_shard(BufReader::new(MultiGzDecoder::new(gz.as_slice()))).unwrap();
        assert_eq!(words["crane"], 5);
        assert_eq!(words["light"], 4);
    }

    #[test]
    fn truncated_gzip_is_an_error() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&b"crane\t2000,5,1\n".repeat(1000))
            .unwrap();
        let gz = encoder.finish().unwrap();
        let cut = &gz[..gz.len() - 8];
        assert!(read_shard(BufReader::new(MultiGzDecoder::new(cut))).is_err());
    }
}
