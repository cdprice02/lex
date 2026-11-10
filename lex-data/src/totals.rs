//! Ngrams totals: for one language, every word that passes `is_valid_word` at a length of 3 to 21
//! with its count summed over all years, before the Dictionary filter. A Corpus is built from
//! them without the network (ADR 0013).
//!
//! One file per length at `totals/{code}/{N}.bin` in the Corpus cache format under the magic
//! `LEXT`, and a `meta.json` (snapshot, shard count, per-file checksums) written last. The set
//! is written into a temporary directory and renamed into place, so a set is either complete
//! or absent: a missing `meta.json`, or a file that disagrees with it, makes the whole set
//! absent and it is downloaded again, never used.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::data_dir::{DataDir, remove_if_present};
use crate::error::LexDataError;
use crate::header::{HEADER_LEN, Header, Kind};
use crate::language::Language;
use crate::le::array_at;
use crate::ngrams::SNAPSHOT;
use crate::store::{encode_records, hex};
use crate::wiktionary;

/// Name of the file, last written, that makes a set of totals complete.
const META_FILE: &str = "meta.json";

/// Totals by word length: each length's words and their summed counts.
pub type ByLength = HashMap<usize, HashMap<String, u64>>;

/// One totals file as the `meta.json` records it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TotalsFile {
    /// Number of records in the file.
    pub records: u64,
    /// Hex SHA-256 of the file's records region.
    pub sha256: String,
}

/// The `meta.json` of a set of totals.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TotalsMeta {
    /// The ngrams release the totals were summed from.
    pub snapshot: String,
    /// Shards the language has in that release, all of which were read.
    pub shard_count: u32,
    /// Each length's file, keyed by word length.
    pub files: BTreeMap<usize, TotalsFile>,
}

/// What is on disk for a language's totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inspection {
    /// Nothing is cached.
    Absent,
    /// A directory exists but is not a complete set, for this reason.
    Incomplete(String),
    /// A complete set.
    Complete(TotalsMeta),
}

/// A cached set of Ngrams totals, as a frontend lists it.
#[derive(Debug, Clone)]
pub struct TotalsRecord {
    /// The language of the totals.
    pub language: Language,
    /// The directory holding the files.
    pub path: PathBuf,
    /// Bytes of every file, `meta.json` included.
    pub size_bytes: u64,
    /// Word lengths with a file.
    pub lengths: usize,
    /// Words over every length.
    pub words: u64,
    /// Shards the totals were summed from.
    pub shard_count: u32,
    /// The ngrams release.
    pub snapshot: String,
}

/// Size in bytes of one record of length `n`.
const fn record_size(n: usize) -> usize {
    n * std::mem::size_of::<u32>() + std::mem::size_of::<u64>()
}

/// Writes a complete set of totals for `lang`, all or nothing, replacing any set already there.
/// Nothing is written for empty `by_length`.
pub fn write(dir: &DataDir, lang: Language, by_length: &ByLength) -> anyhow::Result<()> {
    if by_length.values().all(HashMap::is_empty) {
        return Ok(());
    }
    let root = dir.totals_root();
    std::fs::create_dir_all(&root)
        .with_context(|| format!("creating the totals directory {}", root.display()))?;
    remove_leftovers(dir, lang)?;
    let prefix = leftover_prefix(lang);
    let tmp = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir_in(&root)
        .with_context(|| format!("creating a temporary directory in {}", root.display()))?;
    let mut files = BTreeMap::new();
    for (&n, words) in by_length.iter().filter(|(_, words)| !words.is_empty()) {
        let records = encode_records(n, words);
        let header = Header::for_kind(Kind::Totals, n, record_size(n), &records)?;
        let path = tmp.path().join(format!("{n}.bin"));
        let mut file = std::fs::File::create(&path)
            .with_context(|| format!("writing totals {}", path.display()))?;
        file.write_all(&header.encode()?)?;
        file.write_all(&records)?;
        file.flush()?;
        files.insert(
            n,
            TotalsFile {
                records: u64::try_from(header.record_count)?,
                sha256: header.checksum_hex(),
            },
        );
    }
    let meta = TotalsMeta {
        snapshot: SNAPSHOT.to_owned(),
        shard_count: lang.shard_count(),
        files,
    };
    std::fs::write(
        tmp.path().join(META_FILE),
        serde_json::to_vec_pretty(&meta)?,
    )
    .context("writing the totals meta.json")?;
    let target = dir.totals_dir(lang);
    remove_if_present(&target)?;
    std::fs::rename(tmp.keep(), &target)
        .with_context(|| format!("moving the totals into {}", target.display()))
}

/// Prefix of the temporary directories a write of `lang`'s totals uses.
fn leftover_prefix(lang: Language) -> String {
    format!(".{}-", lang.lang_code())
}

/// The temporary directories an interrupted write of `lang`'s totals left behind.
fn leftovers(dir: &DataDir, lang: Language) -> Vec<PathBuf> {
    let prefix = leftover_prefix(lang);
    std::fs::read_dir(dir.totals_root())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .collect()
}

/// Removes the leftovers of an interrupted write.
fn remove_leftovers(dir: &DataDir, lang: Language) -> anyhow::Result<()> {
    for path in leftovers(dir, lang) {
        remove_if_present(&path)?;
    }
    Ok(())
}

/// What is on disk for `lang`'s totals. Reads the `meta.json` and every file's header, never the
/// records, so a corrupt record shows only when the file is read.
#[must_use]
pub fn inspect(dir: &DataDir, lang: Language) -> Inspection {
    let root = dir.totals_dir(lang);
    if !root.is_dir() {
        return Inspection::Absent;
    }
    match check(dir, lang) {
        Ok(meta) => Inspection::Complete(meta),
        Err(reason) => Inspection::Incomplete(reason),
    }
}

/// The meta of a complete set, or why the set is not one.
fn check(dir: &DataDir, lang: Language) -> Result<TotalsMeta, String> {
    let meta_path = dir.totals_dir(lang).join(META_FILE);
    let bytes = std::fs::read(&meta_path).map_err(|e| format!("{META_FILE} is missing ({e})"))?;
    let meta: TotalsMeta =
        serde_json::from_slice(&bytes).map_err(|e| format!("{META_FILE} is unreadable ({e})"))?;
    if meta.snapshot != SNAPSHOT {
        return Err(format!(
            "they are from snapshot {}, this build reads {SNAPSHOT}",
            meta.snapshot
        ));
    }
    if meta.shard_count != lang.shard_count() {
        return Err(format!(
            "they were summed from {} shards, {lang} has {}",
            meta.shard_count,
            lang.shard_count()
        ));
    }
    if meta.files.is_empty() {
        return Err(format!("{META_FILE} lists no files"));
    }
    for (&n, file) in &meta.files {
        let path = dir.totals_path(lang, n);
        let size = std::fs::metadata(&path)
            .map_err(|e| format!("{n}.bin is missing ({e})"))?
            .len();
        let header = Header::read_file_as(&path, Kind::Totals)
            .map_err(|e| format!("{n}.bin has no readable header ({e:#})"))?;
        let expected = header
            .record_count
            .checked_mul(record_size(n))
            .and_then(|b| b.checked_add(header.records_offset))
            .and_then(|b| u64::try_from(b).ok());
        if usize::from(header.word_len) != n
            || header.checksum_hex() != file.sha256
            || u64::try_from(header.record_count).ok() != Some(file.records)
            || expected != Some(size)
        {
            return Err(format!("{n}.bin disagrees with {META_FILE}"));
        }
    }
    Ok(meta)
}

/// A record of length `n` as a word and its count; `None` when a letter is not a `char`.
fn parse_record(chunk: &[u8], n: usize) -> Option<(String, u64)> {
    let word = (0..n)
        .map(|i| char::from_u32(u32::from_le_bytes(array_at(chunk, i * 4))))
        .collect::<Option<String>>()?;
    Some((word, u64::from_le_bytes(array_at(chunk, n * 4))))
}

/// Reads and verifies the totals file of length `n`: header, records checksum against the
/// header and the `meta.json`, and every record (valid letters, a positive count, counts never
/// increasing). Returns the words in `keep`, all of them when it is `None`.
fn read_length(
    path: &Path,
    n: usize,
    file: &TotalsFile,
    keep: Option<&HashSet<String>>,
) -> anyhow::Result<HashMap<String, u64>> {
    let corrupt = |reason: String| LexDataError::CorruptTotals {
        path: path.to_path_buf(),
        reason,
    };
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let header = Header::parse_as(&bytes, path, Kind::Totals)?;
    let records = bytes
        .get(header.records_offset..)
        .filter(|_| bytes.len() >= HEADER_LEN)
        .ok_or_else(|| corrupt("truncated".to_owned()))?;
    let size = record_size(n);
    if usize::from(header.word_len) != n || records.len() != header.record_count * size {
        return Err(corrupt("its length disagrees with its header".to_owned()).into());
    }
    let sum: [u8; 32] = Sha256::digest(records).into();
    if sum != header.checksum || hex(&sum) != file.sha256 {
        return Err(corrupt("records checksum mismatch".to_owned()).into());
    }
    let mut words = HashMap::new();
    let mut previous = u64::MAX;
    for (index, chunk) in records.chunks_exact(size).enumerate() {
        let (word, count) = parse_record(chunk, n)
            .ok_or_else(|| corrupt(format!("record {index} holds an invalid letter")))?;
        if count == 0 || count > previous {
            return Err(corrupt(format!(
                "record {index}: counts must be positive and non-increasing"
            ))
            .into());
        }
        previous = count;
        if keep.is_none_or(|keep| keep.contains(&word)) {
            words.insert(word, count);
        }
    }
    Ok(words)
}

/// The words of a complete set that are in `valid`, by length, after verifying every file.
/// Lengths with no word left are omitted.
///
/// # Errors
///
/// If a file fails verification or cannot be read.
pub fn read_filtered(
    dir: &DataDir,
    lang: Language,
    meta: &TotalsMeta,
    valid: &HashSet<String>,
) -> anyhow::Result<ByLength> {
    let mut by_length = ByLength::new();
    for (&n, file) in &meta.files {
        let words = read_length(&dir.totals_path(lang, n), n, file, Some(valid))?;
        if !words.is_empty() {
            by_length.insert(n, words);
        }
    }
    Ok(by_length)
}

/// Whether the file of length `n` holds any word of `valid`, stopping at the first. Reads
/// records as a stream and does not verify them.
fn holds_any(path: &Path, n: usize, valid: &HashSet<String>) -> anyhow::Result<bool> {
    let mut reader = BufReader::new(
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?,
    );
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header)?;
    let mut chunk = vec![0u8; record_size(n)];
    while reader.read_exact(&mut chunk).is_ok() {
        if parse_record(&chunk, n).is_some_and(|(word, _)| valid.contains(&word)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The lengths a Corpus built from this set under `valid` would have.
///
/// # Errors
///
/// If a file cannot be read.
pub fn lengths_after_filter(
    dir: &DataDir,
    lang: Language,
    meta: &TotalsMeta,
    valid: &HashSet<String>,
) -> anyhow::Result<Vec<usize>> {
    let mut lengths = Vec::new();
    for &n in meta.files.keys() {
        if holds_any(&dir.totals_path(lang, n), n, valid)? {
            lengths.push(n);
        }
    }
    Ok(lengths)
}

impl DataDir {
    /// The cached Ngrams totals of `lang`, or `None` when none are cached or the set is not
    /// complete (see [`inspect`]). Reads headers only; nothing is fetched.
    ///
    /// # Errors
    ///
    /// If a file's size cannot be read.
    pub fn totals(&self, lang: Language) -> anyhow::Result<Option<TotalsRecord>> {
        let Inspection::Complete(meta) = inspect(self, lang) else {
            return Ok(None);
        };
        let path = self.totals_dir(lang);
        let mut size_bytes = std::fs::metadata(path.join(META_FILE))?.len();
        for &n in meta.files.keys() {
            size_bytes += std::fs::metadata(self.totals_path(lang, n))?.len();
        }
        Ok(Some(TotalsRecord {
            language: lang,
            path,
            size_bytes,
            lengths: meta.files.len(),
            words: meta.files.values().map(|f| f.records).sum(),
            shard_count: meta.shard_count,
            snapshot: meta.snapshot,
        }))
    }

    /// The word lengths a Corpus of `lang` has once built, read from complete cached totals and
    /// the cached Dictionary: those lengths where some total word is in the Dictionary. `None`
    /// when there are no complete totals or no Dictionary, so the answer is not known offline.
    ///
    /// # Errors
    ///
    /// If the Dictionary or a totals file cannot be read.
    pub fn corpus_lengths_from_totals(&self, lang: Language) -> anyhow::Result<Option<Vec<usize>>> {
        let Inspection::Complete(meta) = inspect(self, lang) else {
            return Ok(None);
        };
        if !self.dict_path(lang).exists() {
            return Ok(None);
        }
        let valid = wiktionary::load_valid_words(lang, self.as_ref())?;
        Ok(Some(lengths_after_filter(self, lang, &meta, &valid)?))
    }

    /// Removes the cached Ngrams totals of `lang`, and any directory an interrupted write left,
    /// returning each path removed. Nothing cached removes nothing and succeeds. Neither `clear`
    /// nor `clear_dict` calls this: totals do not depend on the Dictionary.
    ///
    /// # Errors
    ///
    /// If a file exists but cannot be removed.
    pub fn clear_totals(&self, lang: Language) -> anyhow::Result<Vec<PathBuf>> {
        let mut removed = Vec::new();
        for path in std::iter::once(self.totals_dir(lang)).chain(leftovers(self, lang)) {
            if remove_if_present(&path)? {
                removed.push(path);
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ByLength {
        let mut five = HashMap::new();
        five.insert("crane".to_owned(), 300u64);
        five.insert("stare".to_owned(), 300);
        five.insert("zzzzz".to_owned(), 7);
        let mut three = HashMap::new();
        three.insert("ace".to_owned(), 9u64);
        [(5, five), (3, three)].into()
    }

    fn dir() -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    fn valid(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn written_totals_are_complete_and_read_back_filtered() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let Inspection::Complete(meta) = inspect(&dir, Language::Russian) else {
            panic!("expected a complete set");
        };
        assert_eq!(meta.snapshot, SNAPSHOT);
        assert_eq!(meta.shard_count, 2);
        assert_eq!(meta.files.keys().copied().collect::<Vec<_>>(), [3, 5]);
        let got = read_filtered(
            &dir,
            Language::Russian,
            &meta,
            &valid(&["crane", "ace", "other"]),
        )
        .unwrap();
        assert_eq!(got[&5], HashMap::from([("crane".to_owned(), 300)]));
        assert_eq!(got[&3], HashMap::from([("ace".to_owned(), 9)]));
    }

    #[test]
    fn totals_files_carry_the_totals_magic_and_ties_break_by_word() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let bytes = std::fs::read(dir.totals_path(Language::Russian, 5)).unwrap();
        assert_eq!(&bytes[0..4], b"LEXT");
        let first: String = (0..5)
            .map(|i| char::from_u32(u32::from_le_bytes(array_at(&bytes[64..], i * 4))).unwrap())
            .collect();
        assert_eq!(first, "crane", "crane sorts before stare at equal counts");
    }

    #[test]
    fn the_corpus_reader_rejects_a_totals_file() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let err = Header::read_file(&dir.totals_path(Language::Russian, 5)).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<LexDataError>(),
            Some(LexDataError::WrongCacheKind { .. })
        ));
    }

    #[test]
    fn nothing_cached_is_absent_and_empty_totals_write_nothing() {
        let (_tmp, dir) = dir();
        assert_eq!(inspect(&dir, Language::Russian), Inspection::Absent);
        write(&dir, Language::Russian, &ByLength::new()).unwrap();
        assert_eq!(inspect(&dir, Language::Russian), Inspection::Absent);
        assert!(dir.totals(Language::Russian).unwrap().is_none());
    }

    #[test]
    fn a_set_without_its_meta_is_absent_to_every_reader() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        std::fs::remove_file(dir.totals_dir(Language::Russian).join(META_FILE)).unwrap();
        assert!(matches!(
            inspect(&dir, Language::Russian),
            Inspection::Incomplete(_)
        ));
        assert!(dir.totals(Language::Russian).unwrap().is_none());
        assert!(
            dir.corpus_lengths_from_totals(Language::Russian)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_missing_or_changed_file_makes_the_set_incomplete() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let path = dir.totals_path(Language::Russian, 3);
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        // the flipped record is not caught by the header-level check, but reading it is
        let Inspection::Complete(meta) = inspect(&dir, Language::Russian) else {
            panic!("headers still agree");
        };
        let err = read_filtered(&dir, Language::Russian, &meta, &valid(&["ace"])).unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        // a rewritten file with another header does not agree with the meta
        write(&dir, Language::Russian, &sample()).unwrap();
        let mut other = sample();
        other.get_mut(&3).unwrap().insert("bee".to_owned(), 1);
        let replacement = tempfile::tempdir().unwrap();
        let other_dir = DataDir::new(replacement.path());
        write(&other_dir, Language::Russian, &other).unwrap();
        std::fs::copy(other_dir.totals_path(Language::Russian, 3), &path).unwrap();
        assert!(matches!(
            inspect(&dir, Language::Russian),
            Inspection::Incomplete(_)
        ));
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            inspect(&dir, Language::Russian),
            Inspection::Incomplete(_)
        ));
    }

    #[test]
    fn a_set_from_another_snapshot_or_shard_count_is_incomplete() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let meta_path = dir.totals_dir(Language::Russian).join(META_FILE);
        let text = std::fs::read_to_string(&meta_path).unwrap();
        std::fs::write(&meta_path, text.replace(SNAPSHOT, "19990101")).unwrap();
        assert!(matches!(
            inspect(&dir, Language::Russian),
            Inspection::Incomplete(_)
        ));
        std::fs::write(&meta_path, &text).unwrap();
        assert!(matches!(
            inspect(&dir, Language::Russian),
            Inspection::Complete(_)
        ));
        // the same files claimed by a language with another shard count
        std::fs::rename(
            dir.totals_dir(Language::Russian),
            dir.totals_dir(Language::French),
        )
        .unwrap();
        assert!(matches!(
            inspect(&dir, Language::French),
            Inspection::Incomplete(_)
        ));
    }

    #[test]
    fn an_interrupted_write_is_never_a_set_and_the_next_write_removes_it() {
        let (_tmp, dir) = dir();
        let leftover = dir.totals_root().join(".rus-abc");
        std::fs::create_dir_all(&leftover).unwrap();
        std::fs::write(leftover.join("5.bin"), b"partial").unwrap();
        assert_eq!(inspect(&dir, Language::Russian), Inspection::Absent);
        write(&dir, Language::Russian, &sample()).unwrap();
        assert!(!leftover.exists());
        assert!(dir.totals(Language::Russian).unwrap().is_some());
    }

    #[test]
    fn a_rewrite_replaces_the_set() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let only_three: ByLength = [(3, sample().remove(&3).unwrap())].into();
        write(&dir, Language::Russian, &only_three).unwrap();
        let record = dir.totals(Language::Russian).unwrap().unwrap();
        assert_eq!(record.lengths, 1);
        assert_eq!(record.words, 1);
        assert!(!dir.totals_path(Language::Russian, 5).exists());
    }

    #[test]
    fn lengths_after_filter_are_those_with_a_dictionary_word() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        let Inspection::Complete(meta) = inspect(&dir, Language::Russian) else {
            panic!("expected a complete set");
        };
        let lengths =
            lengths_after_filter(&dir, Language::Russian, &meta, &valid(&["stare"])).unwrap();
        assert_eq!(lengths, [5]);
    }

    #[test]
    fn clear_totals_removes_the_set_and_a_missing_one_removes_nothing() {
        let (_tmp, dir) = dir();
        assert!(dir.clear_totals(Language::Russian).unwrap().is_empty());
        write(&dir, Language::Russian, &sample()).unwrap();
        let removed = dir.clear_totals(Language::Russian).unwrap();
        assert_eq!(removed, [dir.totals_dir(Language::Russian)]);
        assert_eq!(inspect(&dir, Language::Russian), Inspection::Absent);
    }

    #[test]
    fn clear_and_clear_dict_keep_the_totals() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        std::fs::create_dir_all(dir.dict_path(Language::Russian).parent().unwrap()).unwrap();
        std::fs::write(dir.dict_path(Language::Russian), "crane\n").unwrap();
        dir.clear(Language::Russian, None).unwrap();
        dir.clear_dict(Language::Russian).unwrap();
        assert!(dir.totals(Language::Russian).unwrap().is_some());
    }

    #[test]
    fn corpus_lengths_come_from_the_totals_and_the_cached_dictionary() {
        let (_tmp, dir) = dir();
        write(&dir, Language::Russian, &sample()).unwrap();
        assert!(
            dir.corpus_lengths_from_totals(Language::Russian)
                .unwrap()
                .is_none(),
            "no Dictionary, so the lengths are unknown"
        );
        std::fs::create_dir_all(dir.dict_path(Language::Russian).parent().unwrap()).unwrap();
        std::fs::write(dir.dict_path(Language::Russian), "ace\ncrane\n").unwrap();
        assert_eq!(
            dir.corpus_lengths_from_totals(Language::Russian).unwrap(),
            Some(vec![3, 5])
        );
        std::fs::write(dir.dict_path(Language::Russian), "ace\n").unwrap();
        assert_eq!(
            dir.corpus_lengths_from_totals(Language::Russian).unwrap(),
            Some(vec![3])
        );
    }
}
