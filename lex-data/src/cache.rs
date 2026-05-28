use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::fetch::fetch_all;
use crate::language::Language;
use crate::word::{Word, WordSet};

const MIN_FREQUENCY: u64 = 1;

pub fn cache_path(data_dir: &Path, lang: Language, n: usize) -> PathBuf {
    data_dir.join(lang.cache_dir()).join(format!("{n}.csv"))
}

/// Fast path: disk hit → single CSV parse into WordSet<N>.
/// Slow path (cache miss): fetches the full corpus, writes all length buckets,
/// then reads N. Subsequent requests for any length of this language are fast-path.
pub async fn get<const N: usize>(data_dir: &Path, lang: Language) -> anyhow::Result<WordSet<N>> {
    ensure(data_dir, lang, N).await?;
    read(data_dir, lang)
}

/// Writes every length bucket returned by fetch_all to disk.
pub fn put(
    data_dir: &Path,
    lang: Language,
    by_length: &HashMap<usize, HashMap<String, u64>>,
) -> anyhow::Result<()> {
    for (&n, words) in by_length {
        write_length(n, data_dir, lang, words)?;
    }
    Ok(())
}

/// Removes the N-length file (Some(n)) or the entire language directory (None).
pub fn invalidate(data_dir: &Path, lang: Language, n: Option<usize>) -> anyhow::Result<()> {
    match n {
        Some(n) => {
            std::fs::remove_file(cache_path(data_dir, lang, n)).ok();
        }
        None => {
            std::fs::remove_dir_all(data_dir.join(lang.cache_dir())).ok();
        }
    }
    Ok(())
}

async fn ensure(data_dir: &Path, lang: Language, n: usize) -> anyhow::Result<()> {
    if !cache_path(data_dir, lang, n).exists() {
        eprintln!("Cache miss — fetching full {} corpus...", lang);
        let by_length = fetch_all(lang).await?;
        put(data_dir, lang, &by_length)?;
        eprintln!(
            "Cached {} corpus ({} length buckets)",
            lang,
            by_length.len()
        );
    }
    Ok(())
}

fn read<const N: usize>(data_dir: &Path, lang: Language) -> anyhow::Result<WordSet<N>> {
    // TODO: memmap2 zero-allocation path: mmap the CSV, scan for '\n'/',' boundaries,
    // use Word::try_from_ascii_bytes() to fill [char; N] directly.
    let path = cache_path(data_dir, lang, N);
    let contents = std::fs::read_to_string(&path)?;
    let mut frequencies = HashMap::new();
    for line in contents.lines() {
        let Some((word_str, freq_str)) = line.split_once(',') else {
            continue;
        };
        let Ok(word) = Word::<N>::try_from(word_str) else {
            continue;
        };
        let Ok(freq) = freq_str.parse::<u64>() else {
            continue;
        };
        frequencies.insert(word, freq);
    }
    Ok(WordSet { frequencies })
}

fn write_length(
    n: usize,
    data_dir: &Path,
    lang: Language,
    words: &HashMap<String, u64>,
) -> anyhow::Result<()> {
    // TODO: fixed-width binary format ([u32; N] chars + u64 freq = N*4+8 bytes/record)
    // once memmap2 is adopted for zero-parse, zero-allocation loading.
    let path = cache_path(data_dir, lang, n);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut entries: Vec<(&str, u64)> = words
        .iter()
        .filter(|(_, freq)| **freq >= MIN_FREQUENCY)
        .map(|(w, f)| (w.as_str(), *f))
        .collect();
    entries.sort_unstable_by_key(|e| std::cmp::Reverse(e.1));
    let mut writer = std::io::BufWriter::new(std::fs::File::create(&path)?);
    for (word, freq) in entries {
        writeln!(writer, "{word},{freq}")?;
    }
    Ok(())
}
