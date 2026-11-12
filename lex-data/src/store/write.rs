//! Encoding and writing Corpus files.

use std::collections::HashMap;
use std::io::{BufWriter, Write};

use crate::data_dir::DataDir;
use crate::header::{Header, record_size};
use crate::language::Language;

/// The records region for the words of one length: most frequent first, equal counts in word
/// order, so the same words and counts always encode to the same bytes.
pub fn encode_records(words: &HashMap<String, u64>) -> Vec<u8> {
    let mut entries: Vec<(&str, u64)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    entries.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    encode_entries(entries)
}

/// The records region for `entries` in the order given.
pub fn encode_entries<'a>(entries: impl IntoIterator<Item = (&'a str, u64)>) -> Vec<u8> {
    let mut records = Vec::new();
    for (word, freq) in entries {
        for ch in word.chars() {
            records.extend_from_slice(&u32::from(ch).to_le_bytes());
        }
        records.extend_from_slice(&freq.to_le_bytes());
    }
    records
}

/// Writes the Corpus file of `lang` at length `n` from an encoded records region.
pub fn write_records(
    dir: &DataDir,
    lang: Language,
    n: usize,
    records: &[u8],
) -> anyhow::Result<()> {
    let path = dir.ngrams_path(lang, n);
    std::fs::create_dir_all(dir.ngrams_dir(lang))?;
    let header = Header::for_records(n, record_size(n), records)?;
    let mut writer = BufWriter::new(std::fs::File::create(&path)?);
    writer.write_all(&header.encode()?)?;
    writer.write_all(records)?;
    writer.flush()?;
    Ok(())
}

/// Writes the Cache for `lang` at length `n`, most frequent word first, equal counts in word
/// order.
pub fn write_bucket(
    dir: &DataDir,
    lang: Language,
    n: usize,
    words: &HashMap<String, u64>,
) -> anyhow::Result<()> {
    write_records(dir, lang, n, &encode_records(words))
}
