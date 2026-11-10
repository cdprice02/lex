//! Reading the cached Corpora from their headers, never fetching, as plain records for any
//! frontend. Listing never hashes the records; [`DataDir::verify_corpus`] does.

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Context;
use sha2::{Digest, Sha256};
use strum::VariantArray;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::data_dir::DataDir;
use crate::header::{HEADER_LEN, Header, Inspected, VERSION};
use crate::language::Language;
use crate::{MAX_WORD_LENGTH, MIN_SUPPORTED_WORDS, MIN_WORD_LENGTH};

/// What a Cache's header (and its age against its Dictionary) says about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusStatus {
    /// A readable header of this build's format version.
    Ok,
    /// A Legacy cache: no header.
    Legacy,
    /// A header of another format version.
    OldVersion,
    /// A truncated or malformed header, or one naming another word length.
    Unreadable,
    /// Readable, but its file is older than the fetch of the Dictionary that filtered it.
    OlderThanDictionary,
}

impl CorpusStatus {
    /// The status as shown in a listing.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Legacy => "legacy",
            Self::OldVersion => "old version",
            Self::Unreadable => "unreadable",
            Self::OlderThanDictionary => "older than its Dictionary",
        }
    }
}

/// A cached Corpus as found on disk.
#[derive(Debug, Clone)]
pub struct CorpusRecord {
    /// The language of the Corpus.
    pub language: Language,
    /// The word length of the Corpus.
    pub word_length: usize,
    /// The Cache file.
    pub path: PathBuf,
    /// Size of the file in bytes.
    pub size_bytes: u64,
    /// The header's format version; `None` for a Legacy or unreadable file.
    pub format_version: Option<u16>,
    /// The header's word count; `None` when the header is not readable.
    pub word_count: Option<usize>,
    /// The Corpus checksum as the header records it, in full; `None` when not readable.
    pub checksum: Option<String>,
    /// What the header says of the file.
    pub status: CorpusStatus,
    /// RFC 3339 time the filtering Dictionary was fetched; `None` when it is missing or has no
    /// Provenance.
    pub dict_fetched_at: Option<String>,
}

impl CorpusRecord {
    /// Whether the length is Supported (at least `MIN_SUPPORTED_WORDS` words); `None` when the
    /// header is not readable.
    #[must_use]
    pub fn supported(&self) -> Option<bool> {
        self.word_count.map(|count| count >= MIN_SUPPORTED_WORDS)
    }

    /// Whether the Dictionary was fetched after this file was last written, so a rebuild would
    /// filter differently.
    #[must_use]
    pub fn older_than_dictionary(&self) -> bool {
        self.status == CorpusStatus::OlderThanDictionary
    }
}

/// Whether a file last written at `modified` predates the Dictionary fetch `fetched_at`.
fn predates(modified: SystemTime, fetched_at: &str) -> bool {
    OffsetDateTime::parse(fetched_at, &Rfc3339)
        .is_ok_and(|fetched| OffsetDateTime::from(modified) < fetched)
}

impl DataDir {
    /// Every cached Corpus ordered by language (`Language::VARIANTS` order) then length. Reads
    /// headers only; nothing is downloaded or hashed.
    ///
    /// # Errors
    ///
    /// If a cache file or a Dictionary metadata file cannot be read.
    pub fn corpora(&self) -> anyhow::Result<Vec<CorpusRecord>> {
        let mut records = Vec::new();
        for &lang in Language::VARIANTS {
            let dict_fetched_at = self.dict_fetched_at(lang)?;
            for n in MIN_WORD_LENGTH..=MAX_WORD_LENGTH {
                records.extend(self.corpus_with(lang, n, dict_fetched_at.clone())?);
            }
        }
        Ok(records)
    }

    /// The cached Corpus of `lang` at length `n`, or `None` when its file is not on disk.
    ///
    /// # Errors
    ///
    /// If the file or the Dictionary metadata exists but cannot be read.
    pub fn corpus(&self, lang: Language, n: usize) -> anyhow::Result<Option<CorpusRecord>> {
        let dict_fetched_at = self.dict_fetched_at(lang)?;
        self.corpus_with(lang, n, dict_fetched_at)
    }

    /// The Dictionary's fetch time, when it records one.
    fn dict_fetched_at(&self, lang: Language) -> anyhow::Result<Option<String>> {
        Ok(self
            .cached_dict_metadata(lang)?
            .and_then(|m| m.fetched_at().map(str::to_owned)))
    }

    /// [`Self::corpus`] with the Dictionary's fetch time already known.
    fn corpus_with(
        &self,
        lang: Language,
        n: usize,
        dict_fetched_at: Option<String>,
    ) -> anyhow::Result<Option<CorpusRecord>> {
        let path = self.ngrams_path(lang, n);
        let Ok(file_meta) = std::fs::metadata(&path) else {
            return Ok(None);
        };
        let (status, header) = match Header::inspect(&path)? {
            Inspected::Header(h) if usize::from(h.word_len) == n => (CorpusStatus::Ok, Some(h)),
            Inspected::Header(_) | Inspected::Unreadable => (CorpusStatus::Unreadable, None),
            Inspected::Legacy => (CorpusStatus::Legacy, None),
            Inspected::OldVersion(_) => (CorpusStatus::OldVersion, None),
        };
        let stale = status == CorpusStatus::Ok
            && file_meta
                .modified()
                .ok()
                .zip(dict_fetched_at.as_deref())
                .is_some_and(|(modified, fetched)| predates(modified, fetched));
        Ok(Some(CorpusRecord {
            language: lang,
            word_length: n,
            size_bytes: file_meta.len(),
            format_version: header.as_ref().map(|_| VERSION),
            word_count: header.as_ref().map(|h| h.record_count),
            checksum: header.as_ref().map(Header::checksum_hex),
            status: if stale {
                CorpusStatus::OlderThanDictionary
            } else {
                status
            },
            dict_fetched_at,
            path,
        }))
    }

    /// Hashes the records of the cached Corpus of `lang` at length `n` and compares the digest
    /// with its header's checksum. A file whose length disagrees with its header's count does
    /// not verify.
    ///
    /// # Errors
    ///
    /// If the file is missing, a Legacy cache or has an unreadable header.
    pub fn verify_corpus(&self, lang: Language, n: usize) -> anyhow::Result<bool> {
        let path = self.ngrams_path(lang, n);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("reading ngrams file: {}", path.display()))?;
        let header = Header::parse(&bytes, &path)?;
        let Some(records) = bytes
            .get(header.records_offset..)
            .filter(|_| bytes.len() >= HEADER_LEN)
        else {
            return Ok(false);
        };
        Ok(<[u8; 32]>::from(Sha256::digest(records)) == header.checksum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp data dir with `files` written under it.
    fn dir_with(files: &[(&str, &[u8])]) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        for (relative, body) in files {
            let path = tmp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    /// A valid cache file of `count` records of `record_size` bytes at length `n`.
    fn cache(n: usize, count: usize) -> Vec<u8> {
        let record_size = n * 4 + 8;
        let records = vec![7u8; record_size * count];
        let header = Header::for_records(n, record_size, &records).unwrap();
        let mut bytes = header.encode().unwrap().to_vec();
        bytes.extend(records);
        bytes
    }

    #[test]
    fn nothing_cached_lists_nothing() {
        let (_tmp, dir) = dir_with(&[]);
        assert!(dir.corpora().unwrap().is_empty());
        assert!(dir.corpus(Language::English, 5).unwrap().is_none());
    }

    #[test]
    fn records_are_ordered_by_language_then_length() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/rus/4.bin", &cache(4, 1)),
            ("ngrams/eng/6.bin", &cache(6, 1)),
            ("ngrams/eng/5.bin", &cache(5, 1)),
        ]);
        let order: Vec<_> = dir
            .corpora()
            .unwrap()
            .iter()
            .map(|r| (r.language, r.word_length))
            .collect();
        assert_eq!(
            order,
            [
                (Language::English, 5),
                (Language::English, 6),
                (Language::Russian, 4)
            ]
        );
    }

    #[test]
    fn a_header_gives_count_checksum_and_supported_without_hashing() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/5.bin", &cache(5, 200)),
            ("ngrams/eng/6.bin", &cache(6, 199)),
        ]);
        let five = dir.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(five.status, CorpusStatus::Ok);
        assert_eq!(five.word_count, Some(200));
        assert_eq!(five.supported(), Some(true));
        assert_eq!(five.checksum.as_deref().map(str::len), Some(64));
        assert_eq!(five.format_version, Some(VERSION));
        let six = dir.corpus(Language::English, 6).unwrap().unwrap();
        assert_eq!(six.supported(), Some(false));
    }

    #[test]
    fn a_file_without_a_header_is_legacy() {
        let (_tmp, dir) = dir_with(&[("ngrams/eng/5.bin", &[1u8; 100])]);
        let record = dir.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(record.status, CorpusStatus::Legacy);
        assert_eq!(record.word_count, None);
        assert_eq!(record.supported(), None);
    }

    #[test]
    fn another_format_version_is_old() {
        let mut bytes = cache(5, 1);
        bytes[4] = 99;
        let (_tmp, dir) = dir_with(&[("ngrams/eng/5.bin", &bytes)]);
        let record = dir.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(record.status, CorpusStatus::OldVersion);
    }

    #[test]
    fn a_truncated_header_or_wrong_length_is_unreadable() {
        let bytes = cache(5, 1);
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/5.bin", &bytes[..30]),
            ("ngrams/eng/6.bin", &bytes),
        ]);
        for n in [5, 6] {
            let record = dir.corpus(Language::English, n).unwrap().unwrap();
            assert_eq!(record.status, CorpusStatus::Unreadable, "length {n}");
        }
    }

    #[test]
    fn a_dictionary_fetched_after_the_file_marks_it_older() {
        let meta = |at: &str| {
            format!(
                r#"{{"min_word_length":3,"max_word_length":5,"word_count":1,"fetched_at":"{at}"}}"#
            )
        };
        let (_tmp, newer) = dir_with(&[
            ("ngrams/eng/5.bin", &cache(5, 1)),
            ("dicts/eng.txt", b"crane\n"),
            (
                "dicts/eng.meta.json",
                meta("2999-01-01T00:00:00Z").as_bytes(),
            ),
        ]);
        let record = newer.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(record.status, CorpusStatus::OlderThanDictionary);
        assert!(record.older_than_dictionary());
        let (_tmp, older) = dir_with(&[
            ("ngrams/eng/5.bin", &cache(5, 1)),
            ("dicts/eng.txt", b"crane\n"),
            (
                "dicts/eng.meta.json",
                meta("2001-01-01T00:00:00Z").as_bytes(),
            ),
        ]);
        let record = older.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(record.status, CorpusStatus::Ok);
    }

    #[test]
    fn a_dictionary_without_provenance_never_marks_a_corpus_old() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/5.bin", &cache(5, 1)),
            ("dicts/eng.txt", b"crane\n"),
            (
                "dicts/eng.meta.json",
                br#"{"min_word_length":5,"max_word_length":5,"word_count":1}"#,
            ),
        ]);
        let record = dir.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(record.status, CorpusStatus::Ok);
    }

    #[test]
    fn verify_hashes_the_records_and_catches_a_flipped_byte() {
        let mut bytes = cache(5, 3);
        let (_tmp, good) = dir_with(&[("ngrams/eng/5.bin", &bytes)]);
        assert!(good.verify_corpus(Language::English, 5).unwrap());
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let (_tmp, bad) = dir_with(&[("ngrams/eng/5.bin", &bytes)]);
        assert!(!bad.verify_corpus(Language::English, 5).unwrap());
        // Listing from headers alone does not notice.
        let record = bad.corpus(Language::English, 5).unwrap().unwrap();
        assert_eq!(record.status, CorpusStatus::Ok);
    }
}
