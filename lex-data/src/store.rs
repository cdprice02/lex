//! Building, reading and clearing the on-disk Cache.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::Context;
use memmap2::Mmap;
use sha2::{Digest, Sha256};

use crate::data_dir::DataDir;
use crate::error::LexDataError;
use crate::header::Header;
use crate::language::Language;
use crate::ngrams;
use crate::totals;
use crate::wiktionary;
use crate::word::{RawRecord, WordSet};
use crate::{MAX_WORD_LENGTH, MIN_SUPPORTED_WORDS, MIN_WORD_LENGTH};

/// Least summed count a word needs to enter a Corpus.
pub const MIN_FREQUENCY: u64 = 1;

/// Builds the ngrams cache for `lang` at word length `N` if it does not already exist.
/// Ensures the Wiktionary dict is present first, then downloads all ngrams shards keeping only
/// Wiktionary-valid words of supported lengths, and writes every length bucket to disk.
pub fn build_if_missing<const N: usize>(dir: &DataDir, lang: Language) -> anyhow::Result<()> {
    if !dir.ngrams_path(lang, N).exists() {
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
    let path = dir.ngrams_path(lang, N);
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
    if usize::from(header.word_len) != N {
        return Err(corrupt(format!(
            "header says word length {}, expected {N}",
            header.word_len
        ))
        .into());
    }
    let start = header.records_offset;
    let size = <RawRecord<'_, N>>::SIZE;
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
    Ok(WordSet::from_cache(mmap, &path, start, count, limit)?)
}

/// Hex SHA-256 of the records in the cached ngrams file, as recorded in its header.
pub fn corpus_checksum(path: &std::path::Path) -> anyhow::Result<String> {
    Ok(Header::read_file(path)?.checksum_hex())
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Removes the cached ngrams file for `lang` at length `n` (Some) or the entire
/// language ngrams directory (None), returning the path removed; nothing cached removes
/// nothing and succeeds.
pub fn clear(dir: &DataDir, lang: Language, n: Option<usize>) -> anyhow::Result<Vec<PathBuf>> {
    let path = n.map_or_else(|| dir.ngrams_dir(lang), |n| dir.ngrams_path(lang, n));
    if crate::data_dir::remove_if_present(&path)? {
        Ok(vec![path])
    } else {
        Ok(Vec::new())
    }
}

/// Writes one Cache per length bucket of `by_length`.
fn populate(
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

/// The records region for the words of one length: most frequent first, equal counts in word
/// order, so the same words and counts always encode to the same bytes.
pub fn encode_records(n: usize, words: &HashMap<String, u64>) -> Vec<u8> {
    let mut entries: Vec<(&str, u64)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    entries.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let record_size = n * std::mem::size_of::<u32>() + std::mem::size_of::<u64>();
    let mut records = Vec::with_capacity(entries.len() * record_size);
    for (word, freq) in entries {
        for ch in word.chars() {
            records.extend_from_slice(&u32::from(ch).to_le_bytes());
        }
        records.extend_from_slice(&freq.to_le_bytes());
    }
    records
}

/// Writes the Cache for `lang` at length `n`, most frequent word first, equal counts in word
/// order.
fn write_bucket(
    dir: &DataDir,
    lang: Language,
    n: usize,
    words: &HashMap<String, u64>,
) -> anyhow::Result<()> {
    let path = dir.ngrams_path(lang, n);
    std::fs::create_dir_all(dir.ngrams_dir(lang))?;
    let records = encode_records(n, words);
    let record_size = n * std::mem::size_of::<u32>() + std::mem::size_of::<u64>();
    let header = Header::for_records(n, record_size, &records)?;
    let mut writer = BufWriter::new(std::fs::File::create(&path)?);
    writer.write_all(&header.encode()?)?;
    writer.write_all(&records)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::data_dir::DataDir;
    use crate::error::LexDataError;
    use crate::language::Language;
    use crate::word::{Word, WordSet};

    use super::{build_all_via, clear, ensure_supported, populate, read};
    use crate::MIN_SUPPORTED_WORDS;
    use crate::totals;

    fn toy_corpus() -> HashMap<usize, HashMap<String, u64>> {
        let mut five = HashMap::new();
        five.insert("crane".to_string(), 300u64);
        five.insert("stare".to_string(), 200);
        five.insert("light".to_string(), 100);
        five.insert("mount".to_string(), 75);
        five.insert("swipe".to_string(), 25);
        let mut m = HashMap::new();
        m.insert(5, five);
        m
    }

    fn all_words_valid() -> HashSet<String> {
        ["crane", "stare", "light", "mount", "swipe", "ace"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect()
    }

    fn make_dir_with_dict(words: &HashSet<String>) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        let dict_dir = tmp.path().join("dicts");
        std::fs::create_dir_all(&dict_dir).unwrap();
        let mut sorted: Vec<&str> = words.iter().map(std::string::String::as_str).collect();
        sorted.sort_unstable();
        std::fs::write(dir.dict_path(Language::English), sorted.join("\n")).unwrap();
        (tmp, dir)
    }

    #[test]
    fn populate_and_read_roundtrip() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        populate(&dir, Language::English, &toy_corpus()).unwrap();

        let ws: WordSet<5> = read::<5>(&dir, Language::English, None).unwrap();
        assert_eq!(ws.len(), 5);
        assert_eq!(
            ws.frequency(&Word::<5>::try_from("crane").unwrap()),
            Some(300)
        );
        assert_eq!(
            ws.frequency(&Word::<5>::try_from("swipe").unwrap()),
            Some(25)
        );
    }

    #[test]
    fn read_respects_limit() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        populate(&dir, Language::English, &toy_corpus()).unwrap();

        let ws: WordSet<5> = read::<5>(&dir, Language::English, Some(2)).unwrap();
        assert_eq!(ws.len(), 2);
        // written sorted by frequency desc → crane (300), stare (200)
        assert!(ws.contains(&Word::<5>::try_from("crane").unwrap()));
        assert!(ws.contains(&Word::<5>::try_from("stare").unwrap()));
    }

    #[test]
    fn clear_removes_file() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        populate(&dir, Language::English, &toy_corpus()).unwrap();

        assert!(dir.ngrams_path(Language::English, 5).exists());
        clear(&dir, Language::English, Some(5)).unwrap();
        assert!(!dir.ngrams_path(Language::English, 5).exists());
    }

    #[test]
    fn clear_removes_language_dir() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        populate(&dir, Language::English, &toy_corpus()).unwrap();

        let lang_dir = dir.as_ref().join("ngrams").join("eng");
        assert!(lang_dir.exists());
        clear(&dir, Language::English, None).unwrap();
        assert!(!lang_dir.exists());
    }

    fn cache_bytes() -> (tempfile::TempDir, DataDir, std::path::PathBuf, Vec<u8>) {
        let valid = all_words_valid();
        let (tmp, dir) = make_dir_with_dict(&valid);
        populate(&dir, Language::English, &toy_corpus()).unwrap();
        let path = dir.ngrams_path(Language::English, 5);
        let bytes = std::fs::read(&path).unwrap();
        (tmp, dir, path, bytes)
    }

    fn load_err(dir: &DataDir) -> LexDataError {
        match read::<5>(dir, Language::English, None) {
            Err(e) => e.downcast::<LexDataError>().expect("typed error"),
            Ok(_) => panic!("expected the load to fail"),
        }
    }

    /// Rewrites the header checksum so a record-level fault is reached past the checksum.
    fn refresh_checksum(bytes: &mut [u8]) {
        use sha2::{Digest, Sha256};
        let sum = Sha256::digest(&bytes[64..]);
        bytes[20..52].copy_from_slice(&sum);
    }

    #[test]
    fn legacy_file_is_rejected() {
        let (_tmp, dir, path, bytes) = cache_bytes();
        std::fs::write(&path, &bytes[64..]).unwrap();
        assert!(matches!(load_err(&dir), LexDataError::LegacyCache { .. }));
    }

    #[test]
    fn wrong_word_length_is_rejected() {
        let (_tmp, dir, path, mut bytes) = cache_bytes();
        bytes[6] = 6;
        std::fs::write(&path, &bytes).unwrap();
        assert!(load_err(&dir).to_string().contains("word length 6"));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let (_tmp, dir, path, mut bytes) = cache_bytes();
        bytes.push(0);
        std::fs::write(&path, &bytes).unwrap();
        assert!(load_err(&dir).to_string().contains("file is"));
    }

    #[test]
    fn truncated_records_are_rejected() {
        let (_tmp, dir, path, bytes) = cache_bytes();
        std::fs::write(&path, &bytes[..bytes.len() - 5]).unwrap();
        assert!(load_err(&dir).to_string().contains("file is"));
    }

    #[test]
    fn flipped_frequency_bit_fails_checksum() {
        let (_tmp, dir, path, mut bytes) = cache_bytes();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(load_err(&dir).to_string().contains("checksum"));
    }

    #[test]
    fn invalid_scalar_names_file_and_record() {
        let (_tmp, dir, path, mut bytes) = cache_bytes();
        // Record 2, first char: a surrogate is not a valid `char`.
        let off = 64 + 2 * 28;
        bytes[off..off + 4].copy_from_slice(&0xD800u32.to_le_bytes());
        refresh_checksum(&mut bytes);
        std::fs::write(&path, &bytes).unwrap();
        let err = load_err(&dir);
        assert!(
            matches!(err, LexDataError::CorruptRecord { index: 2, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("5.bin"), "{err}");
    }

    #[test]
    fn out_of_order_frequency_names_record() {
        let (_tmp, dir, path, mut bytes) = cache_bytes();
        // Record 1's frequency raised above record 0's.
        let off = 64 + 28 + 20;
        bytes[off..off + 8].copy_from_slice(&1000u64.to_le_bytes());
        refresh_checksum(&mut bytes);
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            load_err(&dir),
            LexDataError::CorruptRecord { index: 1, .. }
        ));
    }

    #[test]
    fn invalid_record_beyond_limit_still_fails() {
        let (_tmp, dir, path, mut bytes) = cache_bytes();
        let off = 64 + 4 * 28;
        bytes[off..off + 4].copy_from_slice(&0xD800u32.to_le_bytes());
        refresh_checksum(&mut bytes);
        std::fs::write(&path, &bytes).unwrap();
        assert!(read::<5>(&dir, Language::English, Some(2)).is_err());
    }

    #[test]
    fn corpus_checksum_is_the_records_sha256() {
        use sha2::{Digest, Sha256};
        let (_tmp, dir, _path, bytes) = cache_bytes();
        assert_eq!(
            dir.ngrams_sha256(Language::English, 5).unwrap(),
            super::hex(&Sha256::digest(&bytes[64..]))
        );
    }

    #[test]
    fn ngrams_count_is_the_header_record_count() {
        let (_tmp, dir, _path, bytes) = cache_bytes();
        let count = (bytes.len() - 64) / (5 * 4 + 8);
        assert_eq!(dir.ngrams_count(Language::English, 5).unwrap(), count);
    }

    #[test]
    fn short_length_is_rejected_naming_the_supported_lengths() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        let big: HashMap<String, u64> = (0..u64::try_from(MIN_SUPPORTED_WORDS).unwrap())
            .map(|i| (format!("w{i:04}"), 1000 - i))
            .collect();
        let mut corpus = toy_corpus();
        corpus.insert(5, big);
        corpus.insert(4, [("abcd".to_string(), 1u64)].into());
        populate(&dir, Language::English, &corpus).unwrap();

        let err = dir
            .load::<4>(Language::English, None)
            .unwrap_err()
            .downcast::<LexDataError>()
            .expect("typed error");
        assert!(
            matches!(
                err,
                LexDataError::UnsupportedLength {
                    length: 4,
                    count: 1,
                    ..
                }
            ),
            "{err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("english"), "{msg}");
        assert!(msg.contains("1 words at length 4"), "{msg}");
        assert!(msg.ends_with("Supported lengths for english: 5"), "{msg}");
    }

    #[test]
    fn exactly_the_minimum_is_supported() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        let words: HashMap<String, u64> = (0..u64::try_from(MIN_SUPPORTED_WORDS).unwrap())
            .map(|i| (format!("w{i:04}"), 1000 - i))
            .collect();
        populate(&dir, Language::English, &[(5, words)].into()).unwrap();
        assert!(ensure_supported(&dir, Language::English, 5).is_ok());
    }

    #[test]
    fn word_iter_is_exact_size() {
        let (_tmp, dir, _path, _bytes) = cache_bytes();
        let ws: WordSet<5> = read::<5>(&dir, Language::English, Some(3)).unwrap();
        assert_eq!(ws.words().len(), 3);
    }

    fn synthetic_totals() -> totals::ByLength {
        let mut five = HashMap::new();
        for (word, count) in [
            ("crane", 300u64),
            ("stare", 300),
            ("light", 300),
            ("mount", 75),
            ("swipe", 25),
            ("qqqqq", 500),
        ] {
            five.insert(word.to_string(), count);
        }
        let three = HashMap::from([("ace".to_string(), 9u64), ("zzz".to_string(), 90)]);
        let eleven = HashMap::from([("notadictword".to_string(), 4u64)]);
        [(5, five), (3, three), (12, eleven)].into()
    }

    fn corpus_files(dir: &DataDir) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<_> = std::fs::read_dir(dir.ngrams_dir(Language::English))
            .map(|entries| {
                entries
                    .map(|e| {
                        let path = e.unwrap().path();
                        (
                            path.file_name().unwrap().to_string_lossy().into_owned(),
                            std::fs::read(&path).unwrap(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    fn no_download(_: &(dyn Fn(u32, u32) + Sync)) -> anyhow::Result<totals::ByLength> {
        anyhow::bail!("the network must not be reached")
    }

    #[test]
    fn a_download_writes_the_totals_and_the_filtered_corpus() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        build_all_via(&dir, Language::English, &|_, _| {}, &|_| {
            Ok(synthetic_totals())
        })
        .unwrap();

        let totals::Inspection::Complete(meta) = totals::inspect(&dir, Language::English) else {
            panic!("the download should leave complete totals");
        };
        assert_eq!(meta.files.keys().copied().collect::<Vec<_>>(), [3, 5, 12]);
        let ws: WordSet<5> = read::<5>(&dir, Language::English, None).unwrap();
        assert_eq!(ws.len(), 5, "qqqqq is not in the Dictionary");
        assert!(!dir.ngrams_path(Language::English, 12).exists());
    }

    #[test]
    fn a_corpus_built_from_totals_is_byte_identical_to_the_downloaded_one() {
        let (_tmp_a, downloaded) = make_dir_with_dict(&all_words_valid());
        build_all_via(&downloaded, Language::English, &|_, _| {}, &|_| {
            Ok(synthetic_totals())
        })
        .unwrap();

        let (_tmp_b, offline) = make_dir_with_dict(&all_words_valid());
        totals::write(&offline, Language::English, &synthetic_totals()).unwrap();
        build_all_via(&offline, Language::English, &|_, _| {}, &no_download).unwrap();

        let from_download = corpus_files(&downloaded);
        assert_eq!(from_download.len(), 2);
        assert_eq!(from_download, corpus_files(&offline));
        assert_eq!(
            downloaded.ngrams_sha256(Language::English, 5).unwrap(),
            offline.ngrams_sha256(Language::English, 5).unwrap()
        );
    }

    #[test]
    fn a_rebuild_is_byte_identical_whatever_the_hash_order() {
        let words: Vec<String> = (0..300).map(|i| format!("w{i:04}")).collect();
        let forward: HashMap<String, u64> = words.iter().map(|w| (w.clone(), 7)).collect();
        let backward: HashMap<String, u64> = words.iter().rev().map(|w| (w.clone(), 7)).collect();
        assert_eq!(
            super::encode_records(5, &forward),
            super::encode_records(5, &backward)
        );
        let records = super::encode_records(5, &forward);
        let first: Vec<u8> = records[..20].to_vec();
        assert_eq!(
            first[..4],
            u32::from('w').to_le_bytes(),
            "ties are in word order"
        );
        assert_eq!(first[4..8], u32::from('0').to_le_bytes());
    }

    #[test]
    fn building_from_totals_writes_only_the_missing_lengths() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        totals::write(&dir, Language::English, &synthetic_totals()).unwrap();
        std::fs::create_dir_all(dir.ngrams_dir(Language::English)).unwrap();
        std::fs::write(
            dir.ngrams_path(Language::English, 5),
            b"from an older build",
        )
        .unwrap();

        build_all_via(&dir, Language::English, &|_, _| {}, &no_download).unwrap();

        assert_eq!(
            std::fs::read(dir.ngrams_path(Language::English, 5)).unwrap(),
            b"from an older build"
        );
        assert!(dir.ngrams_path(Language::English, 3).exists());
    }

    #[test]
    fn incomplete_totals_are_downloaded_again_never_used() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        totals::write(&dir, Language::English, &synthetic_totals()).unwrap();
        std::fs::remove_file(dir.totals_dir(Language::English).join("meta.json")).unwrap();
        let downloads = std::cell::Cell::new(0);
        build_all_via(&dir, Language::English, &|_, _| {}, &|_| {
            downloads.set(downloads.get() + 1);
            Ok(synthetic_totals())
        })
        .unwrap();
        assert_eq!(downloads.get(), 1);
        assert!(matches!(
            totals::inspect(&dir, Language::English),
            totals::Inspection::Complete(_)
        ));
    }

    #[test]
    fn totals_with_a_corrupt_record_are_downloaded_again() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        totals::write(&dir, Language::English, &synthetic_totals()).unwrap();
        let path = dir.totals_path(Language::English, 5);
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        let downloads = std::cell::Cell::new(0);
        build_all_via(&dir, Language::English, &|_, _| {}, &|_| {
            downloads.set(downloads.get() + 1);
            Ok(synthetic_totals())
        })
        .unwrap();
        assert_eq!(downloads.get(), 1);
        assert!(read::<5>(&dir, Language::English, None).is_ok());
    }

    #[test]
    fn a_failed_download_leaves_no_totals_and_no_corpus() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        assert!(build_all_via(&dir, Language::English, &|_, _| {}, &no_download).is_err());
        assert!(!dir.totals_dir(Language::English).exists());
        assert!(!dir.ngrams_dir(Language::English).exists());
    }

    fn offline_message(err: &anyhow::Error) -> String {
        match err.downcast_ref::<crate::error::LexDataError>() {
            Some(crate::error::LexDataError::Offline { .. }) => err.to_string(),
            other => panic!("expected the Offline error, got {other:?}"),
        }
    }

    #[test]
    fn offline_without_a_dictionary_names_the_dictionary_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path()).offline(true);
        let err = build_all_via(&dir, Language::English, &|_, _| {}, &no_download).unwrap_err();
        assert_eq!(
            offline_message(&err),
            "offline: the english Dictionary is not cached; run `lex dict fetch --lang english`"
        );
        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    }

    #[test]
    fn offline_without_totals_never_downloads() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        let dir = dir.offline(true);
        let downloads = std::cell::Cell::new(0);
        let err = build_all_via(&dir, Language::English, &|_, _| {}, &|_| {
            downloads.set(downloads.get() + 1);
            Ok(synthetic_totals())
        })
        .unwrap_err();
        assert!(offline_message(&err).contains("lex corpus build --lang english"));
        assert_eq!(downloads.get(), 0);
        assert!(!dir.totals_dir(Language::English).exists());
        assert!(!dir.ngrams_dir(Language::English).exists());
    }

    #[test]
    fn offline_with_incomplete_totals_never_downloads() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        totals::write(&dir, Language::English, &synthetic_totals()).unwrap();
        std::fs::remove_file(dir.totals_dir(Language::English).join("meta.json")).unwrap();
        let dir = dir.offline(true);
        let err = build_all_via(&dir, Language::English, &|_, _| {}, &no_download).unwrap_err();
        offline_message(&err);
    }

    #[test]
    fn offline_builds_the_corpus_from_cached_totals() {
        let (_tmp, dir) = make_dir_with_dict(&all_words_valid());
        totals::write(&dir, Language::English, &synthetic_totals()).unwrap();
        let dir = dir.offline(true);
        build_all_via(&dir, Language::English, &|_, _| {}, &no_download).unwrap();
        let ws: WordSet<5> = read::<5>(&dir, Language::English, None).unwrap();
        assert_eq!(ws.len(), 5);
    }
}
