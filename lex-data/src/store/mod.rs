//! The on-disk Corpus Cache: building ([`build`]), reading and validating ([`read`]) and
//! encoding and writing ([`write`]) its files, plus the helpers they share.

mod build;
mod read;
mod write;

use std::path::PathBuf;

use crate::data_dir::DataDir;
use crate::language::Language;

pub use build::{build_all, build_if_missing};
pub use read::{corpus_checksum, ensure_supported, read};
pub use write::{encode_entries, encode_records, write_records};

/// Least summed count a word needs to enter a Corpus.
pub const MIN_FREQUENCY: u64 = 1;

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

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::data_dir::DataDir;
    use crate::error::LexDataError;
    use crate::language::Language;
    use crate::word::{Word, WordSet};

    use super::build::{build_all_via, populate};
    use super::{clear, ensure_supported, read};
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
            crate::header::hex(&Sha256::digest(&bytes[64..]))
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
            super::encode_records(&forward),
            super::encode_records(&backward)
        );
        let records = super::encode_records(&forward);
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
