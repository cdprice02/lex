//! The on-disk data directory and the paths inside it.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::error::LexDataError;
use crate::header::Header;
use crate::language::Language;
use crate::profile::{Profile, ProfileData};
use crate::profile_store;
use crate::store;
use crate::wiktionary::{self, DictMetadata};
use crate::word::WordSet;

/// The directory holding every Cache, dictionary, Profile and Opening book.
#[derive(Clone, Debug)]
pub struct DataDir {
    /// The root directory.
    path: PathBuf,
}

impl DataDir {
    /// A data directory rooted at `path`; nothing is created until it is used.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Loads a `WordSet` for `lang` at word length `N`, building the local cache on a miss.
    ///
    /// On a cache miss this downloads the Wiktionary word list (once per language) and all
    /// Google Books Ngrams shards for that language, filters to valid words, and writes
    /// every length bucket to disk before returning. Subsequent calls for any length in the
    /// same language are served from disk without any network access.
    pub fn load<const N: usize>(
        &self,
        lang: Language,
        limit: Option<usize>,
    ) -> anyhow::Result<WordSet<N>> {
        store::build_if_missing::<N>(self, lang)?;
        store::ensure_supported(self, lang, N)?;
        let words = store::read::<N>(self, lang, limit)?;
        words.ensure_alphabet_fits()?;
        Ok(words)
    }

    /// Returns Wiktionary-derived metadata for `lang` (word count, length range).
    /// Downloads the dictionary if not already cached.
    pub fn dict_metadata(&self, lang: Language) -> anyhow::Result<DictMetadata> {
        if !self.dict_path(lang).exists() {
            wiktionary::fetch_dict(lang, &self.path)?;
        }
        wiktionary::load_metadata(lang, &self.path)
    }

    /// Corpus checksum of the cached ngrams file for `lang` at word length `n`: the hex
    /// SHA-256 of its records, read from the header. It identifies the exact word list and
    /// frequencies a run was measured against.
    pub fn ngrams_sha256(&self, lang: Language, n: usize) -> anyhow::Result<String> {
        store::corpus_checksum(&self.ngrams_path(lang, n))
    }

    /// Number of words in the cached Corpus for `lang` at word length `n`, read from the header
    /// without loading the records.
    pub fn ngrams_count(&self, lang: Language, n: usize) -> anyhow::Result<usize> {
        Ok(Header::read_file(&self.ngrams_path(lang, n))?.record_count as usize)
    }

    /// Removes the cached ngrams file for `lang` at length `n` (Some) or the entire
    /// language ngrams directory (None), with the Opening books of the same scope. Does not remove
    /// the Wiktionary dict.
    pub fn clear(&self, lang: Language, n: Option<usize>) -> anyhow::Result<()> {
        store::clear(self, lang, n)?;
        self.clear_books(Some(lang), n)
    }

    /// Loads a Profile: its lists (fetched and checksum-verified on first use) against the
    /// Corpus of its language and length, which is built on a miss. `N` must be the Profile's
    /// word length.
    pub fn load_profile<const N: usize>(&self, profile: Profile) -> anyhow::Result<ProfileData<N>> {
        let spec = profile.spec();
        if spec.word_length != N {
            return Err(LexDataError::WordLengthError {
                expected: spec.word_length,
                got: N,
            }
            .into());
        }
        let corpus = self.load::<N>(spec.language, None)?;
        profile_store::load(
            &self.profile_dir(profile),
            &profile.to_string(),
            &spec,
            &corpus,
            &profile_store::http_get,
        )
    }

    /// Removes a Profile's stored lists so they are fetched again on next use, and the Opening
    /// books built for it.
    pub fn clear_profile(&self, profile: Profile) -> anyhow::Result<()> {
        self.clear_profile_books(&profile.to_string())?;
        std::fs::remove_dir_all(self.profile_dir(profile))
            .with_context(|| format!("removing profile {profile}"))
    }

    /// Directory holding a Profile's `answers.txt`, `guess-only.txt` and `meta.json`.
    #[must_use]
    pub fn profile_dir(&self, profile: Profile) -> PathBuf {
        self.path.join("profiles").join(profile.to_string())
    }

    /// Directory holding the cached ngrams binary files for `lang`.
    #[must_use]
    pub fn ngrams_dir(&self, lang: Language) -> PathBuf {
        self.path.join("ngrams").join(lang.lang_code())
    }

    /// Path to the cached ngrams binary file for `lang` at word length `n`.
    #[must_use]
    pub fn ngrams_path(&self, lang: Language, n: usize) -> PathBuf {
        self.ngrams_dir(lang).join(format!("{n}.bin"))
    }

    /// Path to the cached Wiktionary word list for `lang`.
    #[must_use]
    pub fn dict_path(&self, lang: Language) -> PathBuf {
        self.path
            .join("dicts")
            .join(format!("{}.txt", lang.lang_code()))
    }
}

impl AsRef<Path> for DataDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl From<PathBuf> for DataDir {
    fn from(p: PathBuf) -> Self {
        Self::new(p)
    }
}

impl From<&Path> for DataDir {
    fn from(p: &Path) -> Self {
        Self::new(p)
    }
}
