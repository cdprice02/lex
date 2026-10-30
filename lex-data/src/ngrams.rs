//! Downloading and filtering the Google Books Ngrams V3 shards into word counts.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use anyhow::Context;
use flate2::read::MultiGzDecoder;

use crate::http;
use crate::language::Language;
use crate::parse::parse_ngram_line;
use crate::store::MIN_FREQUENCY;
use crate::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

/// Shards downloaded at once.
const SHARD_CONCURRENCY: u32 = 4;

/// Google Books Ngrams V3 snapshot the corpus is built from. The snapshot is frozen, but
/// recording it keeps run provenance complete.
pub const SNAPSHOT: &str = "20200217";

/// Downloads all Google Books Ngrams V3 shards for `lang` with up to `SHARD_CONCURRENCY`
/// requests in flight at once. Returns the words in `valid`, grouped by char count, for every
/// supported length. Callers write all length buckets on a cache miss so future requests for
/// any length of this language are served from disk.
///
/// Words are filtered as each shard streams in, so memory holds only the kept words rather than
/// every distinct token in the corpus.
///
/// Workers pull shard indices from a shared counter; a shard that fails after its retries stops
/// the others from starting new ones. Counts are summed, so the result does not depend on the
/// order shards finish in.
pub fn fetch(
    lang: Language,
    valid: &HashSet<String>,
) -> anyhow::Result<HashMap<usize, HashMap<String, u64>>> {
    let n = lang.shard_count();
    let agent = http::agent();
    let next = AtomicU32::new(0);
    let failed = AtomicBool::new(false);

    let worker = || -> anyhow::Result<Vec<HashMap<String, u64>>> {
        let mut done = Vec::new();
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
                read_shard(BufReader::new(MultiGzDecoder::new(body)), valid)
            }) {
                Ok(words) => {
                    log::info!("  [{}/{}] done", shard + 1, n);
                    done.push(words);
                }
                Err(e) => {
                    failed.store(true, Ordering::Relaxed);
                    return Err(e.context(format!("fetching shard {shard} for {lang}")));
                }
            }
        }
        Ok(done)
    };

    let results: Vec<_> = thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..SHARD_CONCURRENCY.min(n) {
            workers.push(scope.spawn(worker));
        }
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });

    let mut by_length: HashMap<usize, HashMap<String, u64>> = HashMap::new();
    for shard_map in results
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()
        .with_context(|| format!("fetching ngrams for {lang}"))?
        .into_iter()
        .flatten()
    {
        for (word, count) in shard_map {
            let len = word.chars().count();
            *by_length.entry(len).or_default().entry(word).or_insert(0) += count;
        }
    }
    // a word's total, not a single shard's share of it, decides whether it is kept
    for words in by_length.values_mut() {
        words.retain(|_, &mut count| count >= MIN_FREQUENCY);
    }
    by_length.retain(|_, words| !words.is_empty());
    Ok(by_length)
}

/// Sums the counts of every word in `valid` whose length is supported, in one decompressed
/// shard. Membership is a per-word test, so filtering per shard keeps exactly the words that
/// filtering the merged corpus would.
fn read_shard(
    reader: impl BufRead,
    valid: &HashSet<String>,
) -> std::io::Result<HashMap<String, u64>> {
    let mut acc: HashMap<String, u64> = HashMap::new();
    for line in reader.lines() {
        if let Ok((word, count)) = parse_ngram_line(&line?) {
            let len = word.chars().count();
            if (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&len) && valid.contains(&word) {
                *acc.entry(word).or_insert(0) += count;
            }
        }
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(words: &[&str]) -> HashSet<String> {
        words.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn read_shard_sums_repeated_words_and_skips_invalid_lines() {
        let shard = "crane\t2000,5,1\t2001,7,1\n\
                     crane\t1999,3,1\n\
                     stare_NOUN\t2000,9,1\n\
                     two words\t2000,9,1\n\
                     no tab here\n\
                     light\t2000,4,1\n";
        let words = read_shard(shard.as_bytes(), &valid(&["crane", "stare", "light"])).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words["crane"], 15);
        assert_eq!(words["light"], 4);
    }

    #[test]
    fn read_shard_drops_words_outside_the_dictionary_and_supported_lengths() {
        let shard = "crane\t2000,5,1\n\
                     swipe\t2000,5,1\n\
                     ab\t2000,5,1\n\
                     abcdefghijklmnopqrstuv\t2000,5,1\n";
        let words = read_shard(
            shard.as_bytes(),
            &valid(&["crane", "ab", "abcdefghijklmnopqrstuv"]),
        )
        .unwrap();
        assert_eq!(words.keys().collect::<Vec<_>>(), ["crane"]);
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
        let words = read_shard(
            BufReader::new(MultiGzDecoder::new(gz.as_slice())),
            &valid(&["crane", "light"]),
        )
        .unwrap();
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
        assert!(read_shard(BufReader::new(MultiGzDecoder::new(cut)), &valid(&["crane"])).is_err());
    }
}
