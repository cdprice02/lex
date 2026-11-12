//! The on-disk data directory and the paths inside it.

use std::num::NonZeroUsize;
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
    /// Shard downloads to run at once; `None` leaves the count to the default.
    download_workers: Option<NonZeroUsize>,
    /// Whether a download is forbidden; see [`DataDir::offline`].
    offline: bool,
}

impl DataDir {
    /// A data directory rooted at `path`; nothing is created until it is used.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            download_workers: None,
            offline: false,
        }
    }

    /// This directory with downloads forbidden (`true`) or allowed. Offline, every entry point
    /// that would reach the network returns [`LexDataError::Offline`] before any request;
    /// computed work (an Opening book, a Corpus built from cached Ngrams totals) still runs.
    #[must_use]
    pub const fn offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Whether downloads are forbidden.
    #[must_use]
    pub const fn is_offline(&self) -> bool {
        self.offline
    }

    /// Passes when downloads are allowed; offline, names `item` and the `command` that fetches it.
    ///
    /// # Errors
    ///
    /// [`LexDataError::Offline`] when this directory is offline.
    pub fn require_online(&self, item: &str, command: &str) -> Result<(), LexDataError> {
        if self.offline {
            return Err(LexDataError::Offline {
                item: item.to_owned(),
                command: command.to_owned(),
            });
        }
        Ok(())
    }

    /// [`DataDir::require_online`] for the Dictionary of `lang`.
    pub(crate) fn require_online_dict(&self, lang: Language) -> Result<(), LexDataError> {
        self.require_online(
            &format!("the {lang} Dictionary"),
            &format!("lex dict fetch --lang {lang}"),
        )
    }

    /// This directory with `workers` as the number of shards downloaded at once (capped by the
    /// shard count); `None` leaves the default.
    #[must_use]
    pub const fn with_download_workers(mut self, workers: Option<NonZeroUsize>) -> Self {
        self.download_workers = workers;
        self
    }

    /// The requested number of parallel shard downloads, if any.
    #[must_use]
    pub const fn download_workers(&self) -> Option<NonZeroUsize> {
        self.download_workers
    }

    /// Loads a `WordSet` for `lang` at word length `N`, building the local cache on a miss.
    ///
    /// On a cache miss this downloads the Wiktionary word list (once per language) and all
    /// Google Books Ngrams shards for that language, filters to valid words, and writes
    /// every length bucket to disk before returning. Subsequent calls for any length in the
    /// same language are served from disk without any network access.
    ///
    /// # Errors
    ///
    /// If the downloads or the cache files fail, or the Corpus is not Supported at `N`.
    pub fn load<const N: usize>(
        &self,
        lang: Language,
        limit: Option<usize>,
    ) -> anyhow::Result<WordSet<N>> {
        store::build_if_missing(self, lang, N)?;
        store::ensure_supported(self, lang, N)?;
        let words = store::read::<N>(self, lang, limit)?;
        words.ensure_alphabet_fits()?;
        Ok(words)
    }

    /// Downloads KAIKKI's current Wiktionary extract for `lang`, replacing any cached
    /// Dictionary. Callers that keep a Corpus built from the old one clear it first
    /// (`clear_dict`).
    ///
    /// # Errors
    ///
    /// If the download fails or the files cannot be written.
    pub fn fetch_dict(&self, lang: Language) -> anyhow::Result<()> {
        self.require_online_dict(lang)?;
        wiktionary::fetch_dict(lang, &self.path)
    }

    /// Downloads every Ngrams shard of `lang` and writes its Corpus at every length, fetching
    /// the Dictionary first when absent and replacing any cached Corpus files.
    /// `progress(done, total)` is called from worker threads as shards finish, first with 0.
    ///
    /// # Errors
    ///
    /// If a download or a cache file fails.
    pub fn build_corpus(
        &self,
        lang: Language,
        progress: &(dyn Fn(u32, u32) + Sync),
    ) -> anyhow::Result<()> {
        store::build_all(self, lang, progress)
    }

    /// Returns Wiktionary-derived metadata for `lang` (word count, length range).
    /// Downloads the dictionary if not already cached.
    ///
    /// # Errors
    ///
    /// If the dictionary cannot be downloaded, read or parsed.
    pub fn dict_metadata(&self, lang: Language) -> anyhow::Result<DictMetadata> {
        if !self.dict_path(lang).exists() {
            self.fetch_dict(lang)?;
        }
        wiktionary::load_metadata(lang, &self.path)
    }

    /// Corpus checksum of the cached ngrams file for `lang` at word length `n`: the hex
    /// SHA-256 of its records, read from the header. It identifies the exact word list and
    /// frequencies a run was measured against.
    ///
    /// # Errors
    ///
    /// If the cached ngrams file is missing or its header is invalid.
    pub fn ngrams_sha256(&self, lang: Language, n: usize) -> anyhow::Result<String> {
        store::corpus_checksum(&self.ngrams_path(lang, n))
    }

    /// Number of words in the cached Corpus for `lang` at word length `n`, read from the header
    /// without loading the records.
    ///
    /// # Errors
    ///
    /// If the cached ngrams file is missing or its header is invalid.
    pub fn ngrams_count(&self, lang: Language, n: usize) -> anyhow::Result<usize> {
        Ok(Header::read_file(&self.ngrams_path(lang, n))?.record_count)
    }

    /// Removes the cached ngrams file for `lang` at length `n` (Some) or the entire
    /// language ngrams directory (None), with the Opening books of the same scope, and returns
    /// each path removed; an absent cache removes nothing and succeeds. Does not remove the
    /// Dictionary.
    ///
    /// # Errors
    ///
    /// If a cache file exists but cannot be removed.
    pub fn clear(&self, lang: Language, n: Option<usize>) -> anyhow::Result<Vec<PathBuf>> {
        let mut removed = store::clear(self, lang, n)?;
        removed.extend(self.clear_books(Some(lang), n)?);
        Ok(removed)
    }

    /// Removes the Dictionary of `lang` (its word list and metadata), every Corpus cache of
    /// that language at every length, and their Opening books, and returns each path removed.
    /// Missing pieces are skipped. Other languages are untouched.
    ///
    /// The next fetch takes KAIKKI's current extract, which can change the Corpus.
    ///
    /// # Errors
    ///
    /// If a cache file exists but cannot be removed.
    pub fn clear_dict(&self, lang: Language) -> anyhow::Result<Vec<PathBuf>> {
        let mut removed = Vec::new();
        for path in [
            self.dict_path(lang),
            wiktionary::dict_meta_path(&self.path, lang),
            self.ngrams_dir(lang),
        ] {
            if remove_if_present(&path)? {
                removed.push(path);
            }
        }
        removed.extend(self.clear_books(Some(lang), None)?);
        Ok(removed)
    }

    /// Loads a Profile: its lists (fetched and checksum-verified on first use) against the
    /// Corpus of its language and length, which is built on a miss. `N` must be the Profile's
    /// word length.
    ///
    /// # Errors
    ///
    /// If `N` is not the Profile's word length, or its lists or Corpus cannot be fetched or
    /// verified.
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
        if !self
            .profile_dir(profile)
            .join(profile_store::META_FILE)
            .exists()
        {
            self.require_online(
                &format!("the lists of Profile {profile}"),
                &format!("lex profile fetch --profile {profile}"),
            )?;
        }
        profile_store::load(
            &self.profile_dir(profile),
            &profile.to_string(),
            &spec,
            &corpus,
            &profile_store::http_get,
        )
    }

    /// Removes a Profile's stored lists so they are fetched again on next use, and the Opening
    /// books built for it, returning each path removed. A Profile that is not fetched removes
    /// nothing and succeeds.
    ///
    /// # Errors
    ///
    /// If the Profile's directory exists but cannot be removed.
    pub fn clear_profile(&self, profile: Profile) -> anyhow::Result<Vec<PathBuf>> {
        let mut removed = self.clear_profile_books(&profile.to_string())?;
        let dir = self.profile_dir(profile);
        if remove_if_present(&dir)? {
            removed.push(dir);
        }
        Ok(removed)
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

    /// Directory holding every language's Ngrams totals.
    #[must_use]
    pub fn totals_root(&self) -> PathBuf {
        self.path.join("totals")
    }

    /// Directory holding the Ngrams totals of `lang`.
    #[must_use]
    pub fn totals_dir(&self, lang: Language) -> PathBuf {
        self.totals_root().join(lang.lang_code())
    }

    /// Path to the Ngrams totals file of `lang` at word length `n`.
    #[must_use]
    pub fn totals_path(&self, lang: Language, n: usize) -> PathBuf {
        self.totals_dir(lang).join(format!("{n}.bin"))
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

/// Removes a file or directory tree; `false` when it was not there.
pub fn remove_if_present(path: &Path) -> anyhow::Result<bool> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
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
