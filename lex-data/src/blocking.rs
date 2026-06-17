use std::path::{Path, PathBuf};

use crate::data_dir::DataDir as AsyncDataDir;
use crate::language::Language;
use crate::wiktionary::DictMetadata;
use crate::word::WordSet;

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

/// Synchronous access to a [`DataDir`](crate::DataDir).
///
/// Follows the `reqwest::blocking` pattern: each call creates a tokio runtime internally
/// and runs the async work to completion. Use from synchronous contexts (e.g. the `lex`
/// solver binary). If you are already inside a tokio runtime, use [`DataDir`](crate::DataDir)
/// directly.
///
/// Note: creating a runtime per call is acceptable for infrequent operations (initial data
/// loading). A future improvement is to switch `lex-cli` to `#[tokio::main]` and call the
/// async API directly.
#[derive(Clone, Debug)]
pub struct DataDir(AsyncDataDir);

impl DataDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(AsyncDataDir::new(path))
    }

    pub fn load<const N: usize>(
        &self,
        lang: Language,
        limit: Option<usize>,
    ) -> anyhow::Result<WordSet<N>> {
        runtime()?.block_on(self.0.load::<N>(lang, limit))
    }

    pub fn dict_metadata(&self, lang: Language) -> anyhow::Result<DictMetadata> {
        runtime()?.block_on(self.0.dict_metadata(lang))
    }

    pub fn clear(&self, lang: Language, n: Option<usize>) -> anyhow::Result<()> {
        self.0.clear(lang, n)
    }

    pub fn ngrams_path(&self, lang: Language, n: usize) -> PathBuf {
        self.0.ngrams_path(lang, n)
    }

    pub fn dict_path(&self, lang: Language) -> PathBuf {
        self.0.dict_path(lang)
    }
}

impl AsRef<Path> for DataDir {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
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
