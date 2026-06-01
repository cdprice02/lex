/// Synchronous wrappers over the async cache functions.
/// Follows the `reqwest::blocking` pattern: each call creates a tokio runtime
/// internally, runs the async work to completion, and returns the result.
///
/// Use these from synchronous contexts (e.g. the `lex` solver binary).
/// If you are already inside a tokio runtime, call the async functions directly.
use std::path::Path;

use crate::language::Language;
use crate::word::WordSet;

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

pub fn get<const N: usize>(
    data_dir: &Path,
    lang: Language,
    max: Option<usize>,
) -> anyhow::Result<WordSet<N>> {
    runtime()?.block_on(crate::cache::get(data_dir, lang, max))
}

pub fn invalidate(data_dir: &Path, lang: Language, n: Option<usize>) -> anyhow::Result<()> {
    crate::cache::invalidate(data_dir, lang, n)
}
