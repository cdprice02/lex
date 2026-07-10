use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use async_compression::tokio::bufread::GzipDecoder;
use futures::{StreamExt, TryStreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use crate::language::Language;
use crate::parse::parse_ngram_line;

const SHARD_CONCURRENCY: usize = 4;

/// Downloads all Google Books Ngrams V3 shards for `lang` with up to
/// SHARD_CONCURRENCY requests in flight at once. Returns every word grouped by
/// char count. Callers write all length buckets on a cache miss so future requests
/// for any length of this language are served from disk.
pub(crate) async fn fetch(lang: Language) -> anyhow::Result<HashMap<usize, HashMap<String, u64>>> {
    let n = lang.shard_count();
    let client = Arc::new(reqwest::Client::new());

    let shard_maps: Vec<HashMap<String, u64>> = futures::stream::iter(0..n)
        .map(|shard| {
            let client = Arc::clone(&client);
            async move {
                let url = format!(
                    "https://storage.googleapis.com/books/ngrams/books/20200217/{}/1-{shard:05}-of-{n:05}.gz",
                    lang.lang_code()
                );
                log::debug!("  [{}/{}] {url}", shard + 1, n);
                fetch_shard(&client, &url)
                    .await
                    .with_context(|| format!("fetching shard {shard} for {lang}"))
                // TODO(.tasks/07-pipeline-sync.md): retry with exponential backoff + jitter;
                // lands with the sync rewrite
            }
        })
        .buffer_unordered(SHARD_CONCURRENCY)
        .try_collect()
        .await
        .with_context(|| format!("fetching ngrams for {lang}"))?;

    let mut by_length: HashMap<usize, HashMap<String, u64>> = HashMap::new();
    for shard_map in shard_maps {
        for (word, count) in shard_map {
            let len = word.chars().count();
            *by_length.entry(len).or_default().entry(word).or_insert(0) += count;
        }
    }
    Ok(by_length)
}

async fn fetch_shard(client: &reqwest::Client, url: &str) -> anyhow::Result<HashMap<String, u64>> {
    let response = client.get(url).send().await?.error_for_status()?;
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let reader = StreamReader::new(stream);
    let decoder = GzipDecoder::new(BufReader::new(reader));
    let mut lines = BufReader::new(decoder).lines();
    let mut acc: HashMap<String, u64> = HashMap::new();
    while let Some(line) = lines.next_line().await? {
        if let Ok((word, count)) = parse_ngram_line(&line) {
            *acc.entry(word).or_insert(0) += count;
        }
    }
    Ok(acc)
}
