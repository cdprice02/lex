use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use anyhow::Context;
use flate2::read::MultiGzDecoder;

use crate::http;
use crate::language::Language;
use crate::parse::parse_ngram_line;

const SHARD_CONCURRENCY: usize = 4;

/// Google Books Ngrams V3 snapshot the corpus is built from. The snapshot is frozen, but
/// recording it keeps run provenance complete.
pub const SNAPSHOT: &str = "20200217";

/// Downloads all Google Books Ngrams V3 shards for `lang` with up to SHARD_CONCURRENCY
/// requests in flight at once. Returns every word grouped by char count. Callers write all
/// length buckets on a cache miss so future requests for any length of this language are
/// served from disk.
///
/// Workers pull shard indices from a shared counter; a shard that fails after its retries stops
/// the others from starting new ones. Counts are summed, so the result does not depend on the
/// order shards finish in.
pub(crate) fn fetch(lang: Language) -> anyhow::Result<HashMap<usize, HashMap<String, u64>>> {
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
                read_shard(BufReader::new(MultiGzDecoder::new(body)))
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
        let workers: Vec<_> = (0..SHARD_CONCURRENCY.min(n as usize))
            .map(|_| scope.spawn(worker))
            .collect();
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
    Ok(by_length)
}

/// Sums the counts of every valid word in one decompressed shard.
fn read_shard(reader: impl BufRead) -> std::io::Result<HashMap<String, u64>> {
    let mut acc: HashMap<String, u64> = HashMap::new();
    for line in reader.lines() {
        if let Ok((word, count)) = parse_ngram_line(&line?) {
            *acc.entry(word).or_insert(0) += count;
        }
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

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
