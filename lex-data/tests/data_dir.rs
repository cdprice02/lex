#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]
#![expect(
    missing_docs,
    reason = "an integration test crate: its test names are its documentation"
)]

use lex_data::DataDir;
use lex_data::testing::seed_corpus;
use lex_data::{Language, Word, WordSet};

fn seed_bin(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
    seed_corpus(dir, lang, n, entries);
}

/// `count` distinct five-letter words, "crane" first, in frequency-descending order.
fn five_letter_words(count: usize) -> Vec<(String, u64)> {
    let letters = |i: usize| -> String {
        [i / 676, i / 26 % 26, i % 26]
            .iter()
            .map(|&d| char::from(b'a' + u8::try_from(d).unwrap()))
            .collect()
    };
    std::iter::once("crane".to_string())
        .chain((1..count).map(|i| format!("zz{}", letters(i))))
        .zip((1..=u64::try_from(count).unwrap()).rev())
        .collect()
}

fn seed_five_letter_words(dir: &DataDir, count: usize) {
    let words = five_letter_words(count);
    let entries: Vec<(&str, u64)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    seed_bin(dir, Language::English, 5, &entries);
}

// build_if_missing only downloads when the ngrams file is absent.
// Pre-seeding the binary file lets load() skip the network path entirely.

#[test]
fn load_from_seeded_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_five_letter_words(&dir, 250);

    let ws: WordSet<5> = dir.load(Language::English, None).unwrap();
    assert_eq!(ws.len(), 250);
    assert_eq!(
        ws.frequency(&Word::<5>::try_from("crane").unwrap()),
        Some(250)
    );
}

#[test]
fn load_respects_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_five_letter_words(&dir, 250);

    let ws: WordSet<5> = dir.load(Language::English, Some(2)).unwrap();
    assert_eq!(ws.len(), 2);
    assert!(ws.contains(&Word::<5>::try_from("crane").unwrap()));
}

#[test]
fn clear_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_bin(&dir, Language::English, 5, &[("crane", 300)]);

    assert!(dir.ngrams_path(Language::English, 5).exists());
    dir.clear(Language::English, Some(5)).unwrap();
    assert!(!dir.ngrams_path(Language::English, 5).exists());
}

#[test]
fn clear_removes_language_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_bin(&dir, Language::English, 5, &[("crane", 300)]);
    seed_bin(&dir, Language::English, 3, &[("ace", 100)]);

    let lang_dir = tmp.path().join("ngrams").join("eng");
    assert!(lang_dir.exists());
    dir.clear(Language::English, None).unwrap();
    assert!(!lang_dir.exists());
}

#[test]
fn path_helpers_return_expected_paths() {
    let dir = DataDir::new("/data");
    assert_eq!(
        dir.ngrams_path(Language::English, 5),
        std::path::Path::new("/data/ngrams/eng/5.bin")
    );
    assert_eq!(
        dir.dict_path(Language::English),
        std::path::Path::new("/data/dicts/eng.txt")
    );
}

#[test]
fn clearing_a_profile_removes_its_books_and_keeps_other_books() {
    use lex_data::{BookKey, Profile, ProfileKey, StoredBook};

    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    let key = |profile: Option<&str>| BookKey {
        corpus_sha256: "c".to_owned(),
        profile: profile.map(|name| ProfileKey {
            name: name.to_owned(),
            answers_sha256: "a".to_owned(),
            guess_only_sha256: "g".to_owned(),
        }),
        word_length: 5,
        dictionary_length: 10,
        strategy: vec![],
        solver_digest: "d".to_owned(),
    };
    for k in [key(Some("wordle")), key(Some("wordle-all")), key(None)] {
        dir.write_book(Language::English, &k, &StoredBook::default())
            .unwrap();
    }
    std::fs::create_dir_all(dir.profile_dir(Profile::Wordle)).unwrap();

    dir.clear_profile(Profile::Wordle).unwrap();

    let exists = |k: &BookKey| dir.book_path(Language::English, k).exists();
    assert!(!exists(&key(Some("wordle"))));
    assert!(exists(&key(Some("wordle-all"))));
    assert!(exists(&key(None)));
    assert!(!dir.profile_dir(Profile::Wordle).exists());
}

#[test]
fn max_supported_length_is_capped_by_the_global_maximum() {
    for &language in <Language as strum::VariantArray>::VARIANTS {
        let max = language.max_supported_length();
        assert!((lex_data::MIN_WORD_LENGTH..=lex_data::MAX_WORD_LENGTH).contains(&max));
    }
}

/// Checks each language's compiled-in maximum against the Corpus headers in `LEX_DATA_DIR`.
#[test]
#[ignore = "reads the Corpus caches in LEX_DATA_DIR"]
fn max_supported_length_matches_the_cache_headers() {
    let dir = DataDir::new(std::env::var("LEX_DATA_DIR").expect("LEX_DATA_DIR is set"));
    for &language in <Language as strum::VariantArray>::VARIANTS {
        let largest = (lex_data::MIN_WORD_LENGTH..=lex_data::MAX_WORD_LENGTH)
            .filter(|&n| {
                dir.ngrams_count(language, n)
                    .is_ok_and(|count| count >= lex_data::MIN_SUPPORTED_WORDS)
            })
            .max()
            .unwrap_or(0);
        assert_eq!(language.max_supported_length(), largest, "{language}");
    }
}

fn offline_error(err: &anyhow::Error) -> String {
    match err.downcast_ref::<lex_data::LexDataError>() {
        Some(lex_data::LexDataError::Offline { .. }) => err.to_string(),
        other => panic!("expected the Offline error, got {other:?}"),
    }
}

#[test]
fn offline_load_of_an_uncached_corpus_is_the_offline_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path()).offline(true);
    let err = dir.load::<5>(Language::English, None).unwrap_err();
    assert_eq!(
        offline_error(&err),
        "offline: the english Dictionary is not cached; run `lex dict fetch --lang english`"
    );
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn offline_fetch_dict_and_dict_metadata_are_the_offline_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path()).offline(true);
    offline_error(&dir.fetch_dict(Language::English).unwrap_err());
    offline_error(&dir.dict_metadata(Language::English).unwrap_err());
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn offline_load_profile_without_lists_is_the_offline_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path()).offline(true);
    seed_five_letter_words(&dir, 250);
    let Err(err) = dir.load_profile::<5>(lex_data::Profile::Wordle) else {
        panic!("an offline load without the lists must fail");
    };
    assert_eq!(
        offline_error(&err),
        "offline: the lists of Profile wordle is not cached; run `lex profile fetch --profile wordle`"
    );
    assert!(!dir.profile_dir(lex_data::Profile::Wordle).exists());
}

#[test]
fn offline_still_loads_a_cached_corpus() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path()).offline(true);
    seed_five_letter_words(&dir, 250);
    assert_eq!(dir.load::<5>(Language::English, None).unwrap().len(), 250);
}
