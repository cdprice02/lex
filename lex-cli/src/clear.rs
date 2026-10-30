//! The `lex clear` command.

use lex_data::DataDir;

use crate::cli::ClearArgs;

/// Deletes what `args` names: with `--books` only the Opening books (narrowed by `--lang` and
/// `-l`); otherwise a language's Corpus cache or a Profile's lists, each with the books built
/// from them.
///
/// # Errors
///
/// If a cache file cannot be removed, or none of `--lang`, `--profile` and `--books` is named.
pub fn clear(args: &ClearArgs) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data_dir);
    match (args.books, args.lang, args.profile) {
        (true, lang, _) => dir.clear_books(lang, args.word_length),
        (false, Some(lang), _) => dir.clear(lang, args.word_length),
        (false, None, Some(profile)) => dir.clear_profile(profile),
        (false, None, None) => anyhow::bail!("name --lang, --profile or --books"),
    }
}
