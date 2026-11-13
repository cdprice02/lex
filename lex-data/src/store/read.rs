//! Reading and validating a cached Corpus file.

use std::path::PathBuf;

use anyhow::Context;
use memmap2::Mmap;
use sha2::{Digest, Sha256};

use crate::data_dir::DataDir;
use crate::error::LexDataError;
use crate::header::{Header, record_size};
use crate::language::Language;
use crate::word::WordSet;
use crate::{MAX_WORD_LENGTH, MIN_SUPPORTED_WORDS, MIN_WORD_LENGTH};

/// Rejects a length whose cached Corpus holds fewer than `MIN_SUPPORTED_WORDS` words, naming
/// the lengths of `lang` that do, read from the cache headers.
pub fn ensure_supported(dir: &DataDir, lang: Language, n: usize) -> anyhow::Result<()> {
    let count_at = |len: usize| -> Option<usize> {
        let header = Header::read_file(&dir.ngrams_path(lang, len)).ok()?;
        Some(header.record_count)
    };
    let count = match count_at(n) {
        Some(count) => count,
        // An unreadable header is reported by the load itself; a missing file holds no words.
        None if dir.ngrams_path(lang, n).exists() => return Ok(()),
        None => 0,
    };
    if count >= MIN_SUPPORTED_WORDS {
        return Ok(());
    }
    let supported = (MIN_WORD_LENGTH..=MAX_WORD_LENGTH)
        .filter(|&len| count_at(len).is_some_and(|c| c >= MIN_SUPPORTED_WORDS))
        .collect();
    Err(LexDataError::UnsupportedLength {
        lang,
        length: n,
        count,
        min: MIN_SUPPORTED_WORDS,
        supported,
    }
    .into())
}

/// Reads the cached ngrams binary file for `lang` at word length `N` into a `WordSet`.
///
/// Checks the header, the file length it implies, the records checksum, and every record
/// before returning; a failure names the file (and record index) and never falls back to a
/// partial read.
pub fn read<const N: usize>(
    dir: &DataDir,
    lang: Language,
    limit: Option<usize>,
) -> anyhow::Result<WordSet<N>> {
    let Checked {
        mmap,
        path,
        records_start,
        record_count,
    } = open_checked(dir, lang, N)?;
    Ok(WordSet::from_cache(
        mmap,
        &path,
        records_start,
        record_count,
        limit,
    )?)
}

/// A Corpus file that passed its header, length and checksum checks, mapped and ready for the
/// per-record validation.
struct Checked {
    /// The whole file.
    mmap: Mmap,
    /// Where it was read from, for error messages.
    path: PathBuf,
    /// Offset of the first record.
    records_start: usize,
    /// Number of records.
    record_count: usize,
}

/// Maps the Corpus of `lang` at length `n` and checks its header, file length and records
/// checksum. A run-time length keeps this compiled once, not once per word length; only the
/// per-record validation in [`WordSet::from_cache`] depends on the length at compile time.
fn open_checked(dir: &DataDir, lang: Language, n: usize) -> anyhow::Result<Checked> {
    let path = dir.ngrams_path(lang, n);
    let f = std::fs::File::open(&path)
        .with_context(|| format!("opening ngrams file: {}", path.display()))?;
    // SAFETY: the cache is rewritten only by `populate`, before any reader maps it, and nothing
    // else truncates a mapped file; a concurrent truncation would be undefined behaviour.
    let mmap = unsafe { Mmap::map(&f) }
        .with_context(|| format!("mmapping ngrams file: {}", path.display()))?;
    let header = Header::parse(&mmap, &path)?;
    let corrupt = |reason: String| LexDataError::CorruptCache {
        path: path.clone(),
        reason,
    };
    if usize::from(header.word_len) != n {
        return Err(corrupt(format!(
            "header says word length {}, expected {n}",
            header.word_len
        ))
        .into());
    }
    let start = header.records_offset;
    let size = record_size(n);
    let count = header.record_count;
    let expected_len = count.checked_mul(size).and_then(|b| b.checked_add(start));
    if expected_len != Some(mmap.len()) {
        return Err(corrupt(format!(
            "file is {} bytes, header implies {} records of {size} bytes after a {start}-byte header",
            mmap.len(),
            count
        ))
        .into());
    }
    if <[u8; 32]>::from(Sha256::digest(&mmap[start..])) != header.checksum {
        return Err(corrupt("records checksum mismatch".to_string()).into());
    }
    Ok(Checked {
        mmap,
        path,
        records_start: start,
        record_count: count,
    })
}

/// Hex SHA-256 of the records in the cached ngrams file, as recorded in its header.
pub fn corpus_checksum(path: &std::path::Path) -> anyhow::Result<String> {
    Ok(Header::read_file(path)?.checksum_hex())
}
