//! Fixtures that seed a [`DataDir`] with cache files, for tests that must not reach the network.
//!
//! Every file is written by the same code a real build uses (`store`, `totals`), so a format
//! change moves the fixtures with it. One test,
//! `the_writer_emits_the_documented_layout_and_the_reader_accepts_it` in
//! `tests/corpus_format.rs`, keeps a hand-written copy of the Corpus layout as the independent
//! oracle against the writers drifting together with the reader.
//!
//! A fixture that cannot write its file panics: a test cannot go on without it.
#![expect(
    clippy::expect_used,
    reason = "test fixtures: a failed seed aborts the test that asked for it"
)]

use std::collections::HashMap;

use crate::MIN_SUPPORTED_WORDS;
use crate::data_dir::DataDir;
use crate::language::Language;
use crate::store;
use crate::totals;
use crate::wiktionary;

/// Writes the Corpus of `lang` at length `n` holding `entries` in the order given.
///
/// The order must be frequency-descending for the file to load. Unlike a build, equal counts
/// keep their order.
///
/// # Panics
///
/// If the file cannot be written.
pub fn seed_corpus(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
    store::write_records(
        dir,
        lang,
        n,
        &store::encode_entries(entries.iter().copied()),
    )
    .expect("seeding a Corpus");
}

/// Seeds a Supported Corpus: `head`, then padding.
///
/// The padding is distinct low-frequency words of `n` letters, up to the
/// [`MIN_SUPPORTED_WORDS`] a length needs. The head keeps its frequencies and its order.
///
/// # Panics
///
/// If the file cannot be written.
pub fn seed_supported(dir: &DataDir, lang: Language, n: usize, head: &[(&str, u64)]) {
    let floor = head.iter().map(|&(_, f)| f).min().unwrap_or(1);
    let letter = |digit: usize| char::from(b'a' + u8::try_from(digit % 26).unwrap_or(0));
    let padding: Vec<(String, u64)> = (0..MIN_SUPPORTED_WORDS.saturating_sub(head.len()))
        .map(|i| {
            let tail: String = [i / 676, i / 26, i].into_iter().map(letter).collect();
            let word = format!("{}{tail}", "z".repeat(n.saturating_sub(3)));
            let count = u64::try_from(i).map_or(1, |i| floor.saturating_sub(1 + i).max(1));
            (word, count)
        })
        .collect();
    let entries: Vec<(&str, u64)> = head
        .iter()
        .copied()
        .chain(padding.iter().map(|(word, count)| (word.as_str(), *count)))
        .collect();
    seed_corpus(dir, lang, n, &entries);
}

/// Stands in for the cached Wiktionary list of `lang`, so a report's provenance lookup never
/// downloads: an empty word list with metadata for five-letter words.
///
/// # Panics
///
/// If the files cannot be written.
pub fn seed_dict(dir: &DataDir, lang: Language) {
    let path = dir.dict_path(lang);
    std::fs::create_dir_all(path.parent().expect("a Dictionary path has a parent"))
        .expect("creating the dicts directory");
    std::fs::write(&path, "").expect("writing the word list");
    std::fs::write(
        wiktionary::dict_meta_path(dir.as_ref(), lang),
        r#"{"min_word_length":5,"max_word_length":5,"word_count":8}"#,
    )
    .expect("writing the metadata");
}

/// Writes a complete set of Ngrams totals for `lang`, one length per `lengths` entry.
///
/// # Panics
///
/// If the set cannot be written.
pub fn seed_totals(dir: &DataDir, lang: Language, lengths: &[(usize, &[(&str, u64)])]) {
    let by_length: totals::ByLength = lengths
        .iter()
        .map(|&(n, entries)| {
            let words: HashMap<String, u64> = entries
                .iter()
                .map(|&(word, count)| (word.to_owned(), count))
                .collect();
            (n, words)
        })
        .collect();
    totals::write(dir, lang, &by_length).expect("seeding Ngrams totals");
}
