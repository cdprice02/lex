//! Reading the fetched Profiles from disk, never fetching, as plain records for any frontend.

use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;
use strum::VariantArray;

use crate::data_dir::DataDir;
use crate::language::Language;
use crate::list;
use crate::profile::{ListSpec, Profile};
use crate::profile_store::{ANSWERS_FILE, GUESS_ONLY_FILE, META_FILE};

/// One stored list of a fetched Profile, checked against its compiled-in checksum.
#[derive(Debug, Clone)]
pub struct ProfileListRecord {
    /// The stored file.
    pub path: PathBuf,
    /// The pinned download URLs merged into the list; none for a list derived from others.
    pub source_urls: Vec<String>,
    /// The checksum compiled into this build.
    pub expected_sha256: String,
    /// The checksum of the file on disk; `None` when the file is missing.
    pub actual_sha256: Option<String>,
    /// Words in the file, counted from its lines; 0 when the file is missing.
    pub word_count: usize,
}

impl ProfileListRecord {
    /// Whether the file is present and hashes to the compiled-in checksum.
    #[must_use]
    pub fn verified(&self) -> bool {
        self.actual_sha256.as_deref() == Some(self.expected_sha256.as_str())
    }
}

/// A fetched Profile as found on disk.
#[derive(Debug, Clone)]
pub struct ProfileRecord {
    /// Which Profile.
    pub profile: Profile,
    /// Its directory.
    pub dir: PathBuf,
    /// The language of its Corpus.
    pub language: Language,
    /// The length of its words.
    pub word_length: usize,
    /// RFC 3339 time of the fetch, from `meta.json`.
    pub fetched_at: String,
    /// The Answer list.
    pub answers: ProfileListRecord,
    /// The guess-only list.
    pub guess_only: ProfileListRecord,
    /// Whether its Corpus is cached.
    pub corpus_cached: bool,
}

impl ProfileRecord {
    /// Whether both lists hash to their compiled-in checksums.
    #[must_use]
    pub fn verified(&self) -> bool {
        self.answers.verified() && self.guess_only.verified()
    }
}

/// The part of `meta.json` a listing reads.
#[derive(Deserialize)]
struct MetaFetchTime {
    /// RFC 3339 time of the fetch.
    fetched_at: String,
}

/// The record of the stored list `file` in `dir`, against `spec`.
fn list_record(
    dir: &std::path::Path,
    file: &str,
    spec: &ListSpec,
) -> anyhow::Result<ProfileListRecord> {
    let path = dir.join(file);
    let (actual_sha256, word_count) = match std::fs::read(&path) {
        Ok(bytes) => (
            Some(list::checksum(&bytes)),
            bytes
                .split(|&b| b == b'\n')
                .filter(|l| !l.is_empty())
                .count(),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, 0),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    Ok(ProfileListRecord {
        path,
        source_urls: spec.sources.iter().map(|s| s.url.to_owned()).collect(),
        expected_sha256: spec.sha256.to_owned(),
        actual_sha256,
        word_count,
    })
}

impl DataDir {
    /// Every fetched Profile in `Profile::VARIANTS` order. A Profile whose `meta.json` is absent
    /// is not fetched and is absent here; nothing is downloaded.
    ///
    /// # Errors
    ///
    /// If a fetched Profile's `meta.json` or lists cannot be read or parsed.
    pub fn profiles(&self) -> anyhow::Result<Vec<ProfileRecord>> {
        let mut records = Vec::new();
        for &profile in Profile::VARIANTS {
            records.extend(self.fetched_profile(profile)?);
        }
        Ok(records)
    }

    /// The fetched Profile `profile`, or `None` when it has not been fetched. Never downloads.
    ///
    /// # Errors
    ///
    /// If `meta.json` or a list exists but cannot be read or parsed.
    pub fn fetched_profile(&self, profile: Profile) -> anyhow::Result<Option<ProfileRecord>> {
        let dir = self.profile_dir(profile);
        let meta_path = dir.join(META_FILE);
        let text = match std::fs::read_to_string(&meta_path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", meta_path.display())),
        };
        let meta: MetaFetchTime = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", meta_path.display()))?;
        let spec = profile.spec();
        Ok(Some(ProfileRecord {
            profile,
            language: spec.language,
            word_length: spec.word_length,
            fetched_at: meta.fetched_at,
            answers: list_record(&dir, ANSWERS_FILE, &spec.answers)?,
            guess_only: list_record(&dir, GUESS_ONLY_FILE, &spec.guess_only)?,
            corpus_cached: self.ngrams_path(spec.language, spec.word_length).is_file(),
            dir,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp data dir with `files` written under it.
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

    const META: &str = r#"{"profile":"wordle-all","fetched_at":"2026-05-01T12:34:56Z"}"#;

    #[test]
    fn nothing_fetched_lists_nothing_and_never_fetches() {
        let (tmp, dir) = dir_with(&[]);
        assert!(dir.profiles().unwrap().is_empty());
        assert!(dir.fetched_profile(Profile::Wordle).unwrap().is_none());
        assert!(!tmp.path().join("profiles").exists());
    }

    #[test]
    fn lists_without_meta_json_are_not_fetched() {
        let (_tmp, dir) = dir_with(&[("profiles/wordle/answers.txt", "crane\n")]);
        assert!(dir.profiles().unwrap().is_empty());
    }

    #[test]
    fn a_fetched_profile_reports_counts_and_unverified_checksums() {
        let (_tmp, dir) = dir_with(&[
            ("profiles/wordle/answers.txt", "crane\nslate\n"),
            ("profiles/wordle/guess-only.txt", "aahed\n"),
            ("profiles/wordle/meta.json", META),
            ("ngrams/eng/5.bin", "x"),
        ]);
        let record = dir.fetched_profile(Profile::Wordle).unwrap().unwrap();
        assert_eq!(record.answers.word_count, 2);
        assert_eq!(record.guess_only.word_count, 1);
        assert_eq!(record.fetched_at, "2026-05-01T12:34:56Z");
        assert!(record.corpus_cached);
        assert!(!record.verified());
        assert_eq!(record.answers.source_urls.len(), 1);
        assert_eq!(record.answers.expected_sha256.len(), 64);
        assert_eq!(
            record.answers.actual_sha256.as_deref().map(str::len),
            Some(64)
        );
    }

    #[test]
    fn wordle_all_verifies_an_empty_guess_only_list_and_a_missing_file_does_not() {
        let (_tmp, dir) = dir_with(&[
            ("profiles/wordle-all/guess-only.txt", ""),
            ("profiles/wordle-all/meta.json", META),
        ]);
        let record = dir.fetched_profile(Profile::WordleAll).unwrap().unwrap();
        assert!(record.guess_only.verified());
        assert!(record.guess_only.source_urls.is_empty());
        assert!(!record.answers.verified());
        assert_eq!(record.answers.actual_sha256, None);
        assert!(!record.corpus_cached);
    }

    #[test]
    fn an_unparseable_meta_json_is_an_error_naming_the_file() {
        let (_tmp, dir) = dir_with(&[("profiles/wordle/meta.json", "{")]);
        let error = format!("{:#}", dir.profiles().unwrap_err());
        assert!(error.contains("meta.json"), "{error}");
    }
}
