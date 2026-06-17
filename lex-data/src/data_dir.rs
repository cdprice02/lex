use std::path::{Path, PathBuf};

use crate::language::Language;
use crate::store;
use crate::wiktionary::{self, DictMetadata};
use crate::word::WordSet;

#[derive(Clone, Debug)]
pub struct DataDir {
    path: PathBuf,
}

impl DataDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Loads a WordSet for `lang` at word length `N`, building the local cache on a miss.
    ///
    /// On a cache miss this downloads the Wiktionary word list (once per language) and all
    /// Google Books Ngrams shards for that language, filters to valid words, and writes
    /// every length bucket to disk before returning. Subsequent calls for any length in the
    /// same language are served from disk without any network access.
    pub async fn load<const N: usize>(
        &self,
        lang: Language,
        limit: Option<usize>,
    ) -> anyhow::Result<WordSet<N>> {
        store::build_if_missing::<N>(self, lang).await?;
        store::read::<N>(self, lang, limit)
    }

    /// Returns Wiktionary-derived metadata for `lang` (word count, length range).
    /// Downloads the dictionary if not already cached.
    pub async fn dict_metadata(&self, lang: Language) -> anyhow::Result<DictMetadata> {
        if !self.dict_path(lang).exists() {
            wiktionary::fetch_dict(lang, &self.path).await?;
        }
        wiktionary::load_metadata(lang, &self.path)
    }

    /// Removes the cached ngrams file for `lang` at length `n` (Some) or the entire
    /// language ngrams directory (None). Does not remove the Wiktionary dict.
    pub fn clear(&self, lang: Language, n: Option<usize>) -> anyhow::Result<()> {
        store::clear(self, lang, n)
    }

    /// Path to the cached ngrams CSV for `lang` at word length `n`.
    pub fn ngrams_path(&self, lang: Language, n: usize) -> PathBuf {
        self.path
            .join("ngrams")
            .join(lang.lang_code())
            .join(format!("{n}.csv"))
    }

    /// Path to the cached Wiktionary word list for `lang`.
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
