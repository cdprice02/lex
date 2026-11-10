//! Reading the cached Dictionaries from disk, never fetching, as plain records for any frontend.

use std::io::BufRead;
use std::path::PathBuf;

use anyhow::Context;
use strum::VariantArray;

use crate::data_dir::DataDir;
use crate::language::Language;
use crate::wiktionary::{self, DictMetadata};
use crate::{MAX_WORD_LENGTH, MIN_WORD_LENGTH};

/// A cached Dictionary as found on disk.
#[derive(Debug, Clone)]
pub struct DictRecord {
    /// The language the Dictionary filters.
    pub language: Language,
    /// The word list file.
    pub path: PathBuf,
    /// The metadata file, which may not exist.
    pub meta_path: PathBuf,
    /// Size of the word list file in bytes.
    pub size_bytes: u64,
    /// Words in the file, counted from its lines.
    pub word_count: usize,
    /// Words per Supported length: `(n, count)` for every `n` in
    /// `MIN_WORD_LENGTH..=MAX_WORD_LENGTH`, counted in characters, not bytes.
    pub length_counts: Vec<(usize, usize)>,
    /// Words of a length outside the Supported range.
    pub outside_supported: usize,
    /// What the metadata file recorded; `None` when the file is missing. Its provenance fields
    /// are `None` in caches that predate stamping.
    pub metadata: Option<DictMetadata>,
    /// Word lengths whose Corpus (`ngrams/{code}/{N}.bin`) is cached, ascending.
    pub corpus_lengths: Vec<usize>,
}

impl DictRecord {
    /// The metadata's word count when it differs from the file's line count.
    #[must_use]
    pub fn metadata_mismatch(&self) -> Option<usize> {
        self.metadata
            .as_ref()
            .map(|m| m.word_count)
            .filter(|&count| count != self.word_count)
    }
}

impl DataDir {
    /// Every cached Dictionary in `Language::VARIANTS` order. Reads disk only; a language with
    /// no word list is absent, and nothing is downloaded.
    ///
    /// # Errors
    ///
    /// If a cached word list or metadata file cannot be read or parsed.
    pub fn dicts(&self) -> anyhow::Result<Vec<DictRecord>> {
        let mut records = Vec::new();
        for &lang in Language::VARIANTS {
            records.extend(self.dict(lang)?);
        }
        Ok(records)
    }

    /// The cached Dictionary of `lang`, or `None` when its word list is not on disk. Never
    /// downloads.
    ///
    /// # Errors
    ///
    /// If the word list or metadata file exists but cannot be read or parsed.
    pub fn dict(&self, lang: Language) -> anyhow::Result<Option<DictRecord>> {
        let path = self.dict_path(lang);
        let Ok(file_meta) = std::fs::metadata(&path) else {
            return Ok(None);
        };
        let file = std::fs::File::open(&path)
            .with_context(|| format!("reading dict for {lang} at {}", path.display()))?;
        let mut per_length = [0usize; MAX_WORD_LENGTH + 1];
        let mut word_count = 0;
        let mut outside_supported = 0;
        for line in std::io::BufReader::new(file).lines() {
            let line = line.with_context(|| format!("reading {}", path.display()))?;
            if line.is_empty() {
                continue;
            }
            word_count += 1;
            let length = line.chars().count();
            match per_length.get_mut(length) {
                Some(slot) if length >= MIN_WORD_LENGTH => *slot += 1,
                _ => outside_supported += 1,
            }
        }
        let length_counts = (MIN_WORD_LENGTH..=MAX_WORD_LENGTH)
            .map(|n| (n, per_length.get(n).copied().unwrap_or(0)))
            .collect();
        Ok(Some(DictRecord {
            language: lang,
            meta_path: wiktionary::dict_meta_path(self.as_ref(), lang),
            size_bytes: file_meta.len(),
            word_count,
            length_counts,
            outside_supported,
            metadata: self.cached_dict_metadata(lang)?,
            corpus_lengths: (MIN_WORD_LENGTH..=MAX_WORD_LENGTH)
                .filter(|&n| self.ngrams_path(lang, n).is_file())
                .collect(),
            path,
        }))
    }

    /// The metadata of the cached Dictionary of `lang`, or `None` when its metadata file is
    /// missing. Unlike `dict_metadata`, never downloads.
    ///
    /// # Errors
    ///
    /// If the metadata file exists but cannot be read or parsed.
    pub fn cached_dict_metadata(&self, lang: Language) -> anyhow::Result<Option<DictMetadata>> {
        if !wiktionary::dict_meta_path(self.as_ref(), lang).exists() {
            return Ok(None);
        }
        wiktionary::load_metadata(lang, self.as_ref()).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        for (relative, body) in files {
            let path = tmp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    const META: &str = r#"{"min_word_length":1,"max_word_length":9,"word_count":5,
        "source_url":"https://example.test/x.gz","fetched_at":"2026-05-01T12:34:56Z",
        "source_sha256":"abcdef0123456789"}"#;

    #[test]
    fn nothing_cached_lists_nothing_and_never_fetches() {
        let (_tmp, dir) = dir_with(&[]);
        assert!(dir.dicts().unwrap().is_empty());
        assert!(dir.dict(Language::English).unwrap().is_none());
        assert!(
            dir.cached_dict_metadata(Language::English)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_word_list_without_metadata_lists_with_counts_from_the_file() {
        let (_tmp, dir) = dir_with(&[("dicts/eng.txt", "a\ncat\ncrane\n")]);
        let records = dir.dicts().unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.word_count, 3);
        assert_eq!(record.size_bytes, 12);
        assert!(record.metadata.is_none());
        assert_eq!(record.outside_supported, 1);
        assert_eq!(record.length_counts[0], (3, 1));
        assert_eq!(record.length_counts[2], (5, 1));
        assert_eq!(record.metadata_mismatch(), None);
    }

    #[test]
    fn metadata_without_provenance_reads_as_none() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/eng.txt", "cat\n"),
            (
                "dicts/eng.meta.json",
                r#"{"min_word_length":3,"max_word_length":3,"word_count":1}"#,
            ),
        ]);
        let record = dir.dict(Language::English).unwrap().unwrap();
        let meta = record.metadata.unwrap();
        assert!(meta.source_url().is_none());
        assert!(meta.fetched_at().is_none());
        assert!(meta.source_sha256().is_none());
    }

    #[test]
    fn lengths_count_characters_not_bytes() {
        let (_tmp, dir) = dir_with(&[("dicts/rus.txt", "дом\nслово\nкот\n")]);
        let record = dir.dict(Language::Russian).unwrap().unwrap();
        assert_eq!(record.length_counts[0], (3, 2));
        assert_eq!(record.length_counts[2], (5, 1));
        assert_eq!(record.outside_supported, 0);
    }

    #[test]
    fn a_metadata_count_that_differs_from_the_file_is_reported() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/eng.txt", "cat\ndog\n"),
            ("dicts/eng.meta.json", META),
        ]);
        let record = dir.dict(Language::English).unwrap().unwrap();
        assert_eq!(record.word_count, 2);
        assert_eq!(record.metadata_mismatch(), Some(5));
    }

    #[test]
    fn corpus_lengths_are_the_ngrams_files_present() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/eng.txt", "cat\n"),
            ("ngrams/eng/5.bin", "x"),
            ("ngrams/eng/3.bin", "x"),
            ("ngrams/fre/4.bin", "x"),
        ]);
        let record = dir.dict(Language::English).unwrap().unwrap();
        assert_eq!(record.corpus_lengths, [3, 5]);
    }

    #[test]
    fn records_follow_language_order() {
        let (_tmp, dir) = dir_with(&[("dicts/rus.txt", "дом\n"), ("dicts/eng.txt", "cat\n")]);
        let languages: Vec<_> = dir.dicts().unwrap().iter().map(|r| r.language).collect();
        assert_eq!(languages, [Language::English, Language::Russian]);
    }
}
