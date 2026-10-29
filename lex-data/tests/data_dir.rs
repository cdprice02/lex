#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    missing_docs,
    reason = "an integration test crate: its test names are its documentation"
)]

use std::fs;
use std::io::Write;

use lex_data::DataDir;
use lex_data::{Language, Word, WordSet};

/// Writes a cache file in the documented on-disk format: a 64-byte header (magic, version,
/// word length, header length, record count, records SHA-256) followed by the records.
fn seed_bin(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
    use sha2::{Digest, Sha256};
    let path = dir.ngrams_path(lang, n);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut records = Vec::new();
    for &(word, freq) in entries {
        for ch in word.chars() {
            records.extend_from_slice(&u32::from(ch).to_le_bytes());
        }
        records.extend_from_slice(&freq.to_le_bytes());
    }
    let mut header = [0u8; 64];
    header[0..4].copy_from_slice(b"LEXC");
    header[4..6].copy_from_slice(&1u16.to_le_bytes());
    header[6..8].copy_from_slice(&u16::try_from(n).unwrap().to_le_bytes());
    header[8..12].copy_from_slice(&64u32.to_le_bytes());
    header[12..20].copy_from_slice(&u64::try_from(entries.len()).unwrap().to_le_bytes());
    header[20..52].copy_from_slice(&Sha256::digest(&records));
    let mut f = fs::File::create(path).unwrap();
    f.write_all(&header).unwrap();
    f.write_all(&records).unwrap();
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
