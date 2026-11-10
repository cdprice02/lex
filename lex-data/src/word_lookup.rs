//! Read-only lookups of single words against the cached Dictionary, Corpus and Profile lists,
//! for any frontend. Nothing here fetches or builds, and a cache that is absent reads as `None`.

use std::collections::HashSet;
use std::io::BufRead;

use anyhow::Context;

use crate::data_dir::DataDir;
use crate::language::Language;
use crate::profile::Profile;
use crate::profile_store::{ANSWERS_FILE, GUESS_ONLY_FILE};
use crate::store;
use crate::word::WordSet;

/// The words of a fetched Profile's two stored lists, as normalized strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileWords {
    /// The Answer list.
    pub answers: Vec<String>,
    /// The guess-only words; the Guess pool is these plus the answers.
    pub guess_only: Vec<String>,
}

/// The non-empty lines of the stored list `file` in `dir`.
fn read_lines(path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let file = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    std::io::BufReader::new(file)
        .lines()
        .filter(|line| line.as_ref().map_or(true, |l| !l.is_empty()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("reading {}", path.display()))
}

impl DataDir {
    /// The members of `words` that the cached Dictionary of `lang` holds, or `None` when the
    /// Dictionary is not cached. `words` are in the form the Dictionary stores (normalized).
    /// Streams the word list once and never downloads.
    ///
    /// # Errors
    ///
    /// If the word list exists but cannot be read.
    pub fn dict_members(
        &self,
        lang: Language,
        words: &HashSet<String>,
    ) -> anyhow::Result<Option<HashSet<String>>> {
        let path = self.dict_path(lang);
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let mut found = HashSet::new();
        for line in std::io::BufReader::new(file).lines() {
            let line = line.with_context(|| format!("reading {}", path.display()))?;
            if words.contains(&line) {
                found.insert(line);
            }
        }
        Ok(Some(found))
    }

    /// The cached Corpus of `lang` at length `N`, fully validated, or `None` when it is not
    /// cached. Unlike [`load`](Self::load) it never builds, and it does not reject a length
    /// under `MIN_SUPPORTED_WORDS`: the caller reads the size from the set.
    ///
    /// # Errors
    ///
    /// If the file exists but is damaged, or its Alphabet is too large.
    pub fn cached_corpus<const N: usize>(
        &self,
        lang: Language,
    ) -> anyhow::Result<Option<WordSet<N>>> {
        if !self.ngrams_path(lang, N).is_file() {
            return Ok(None);
        }
        let words = store::read::<N>(self, lang, None)?;
        words.ensure_alphabet_fits()?;
        Ok(Some(words))
    }

    /// The stored lists of a fetched Profile, or `None` when it is not fetched. The lists are
    /// not re-hashed (`profiles` and `fetched_profile` report that) and nothing is downloaded.
    ///
    /// # Errors
    ///
    /// If the Profile is fetched but a list cannot be read.
    pub fn profile_words(&self, profile: Profile) -> anyhow::Result<Option<ProfileWords>> {
        if self.fetched_profile(profile)?.is_none() {
            return Ok(None);
        }
        let dir = self.profile_dir(profile);
        Ok(Some(ProfileWords {
            answers: read_lines(&dir.join(ANSWERS_FILE))?,
            guess_only: read_lines(&dir.join(GUESS_ONLY_FILE))?,
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

    fn set(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn dictionary_members_are_the_requested_words_the_file_holds() {
        let (_tmp, dir) = dir_with(&[("dicts/eng.txt", "crane\nslate\nмир\n")]);
        let found = dir
            .dict_members(Language::English, &set(&["crane", "zzzzz", "мир"]))
            .unwrap()
            .unwrap();
        assert_eq!(found, set(&["crane", "мир"]));
    }

    #[test]
    fn nothing_cached_reads_none_and_creates_nothing() {
        let (tmp, dir) = dir_with(&[]);
        assert!(
            dir.dict_members(Language::English, &set(&["crane"]))
                .unwrap()
                .is_none()
        );
        assert!(dir.cached_corpus::<5>(Language::English).unwrap().is_none());
        assert!(dir.profile_words(Profile::Wordle).unwrap().is_none());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_fetched_profile_gives_its_lists() {
        let (_tmp, dir) = dir_with(&[
            ("profiles/wordle/answers.txt", "crane\nslate\n"),
            ("profiles/wordle/guess-only.txt", "aahed\n"),
            (
                "profiles/wordle/meta.json",
                r#"{"profile":"wordle","fetched_at":"2026-05-01T12:34:56Z"}"#,
            ),
        ]);
        let words = dir.profile_words(Profile::Wordle).unwrap().unwrap();
        assert_eq!(words.answers, ["crane", "slate"]);
        assert_eq!(words.guess_only, ["aahed"]);
    }

    #[test]
    fn a_damaged_corpus_is_an_error_not_a_miss() {
        let (_tmp, dir) = dir_with(&[("ngrams/eng/5.bin", "not a cache")]);
        assert!(dir.cached_corpus::<5>(Language::English).is_err());
    }
}
