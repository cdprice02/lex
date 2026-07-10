use std::fs;
use std::io::Write;

use lex_data::blocking::DataDir;
use lex_data::{Language, Word, WordSet};

fn seed_bin(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
    let path = dir.ngrams_path(lang, n);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = fs::File::create(path).unwrap();
    for &(word, freq) in entries {
        for ch in word.chars() {
            f.write_all(&(ch as u32).to_le_bytes()).unwrap();
        }
        f.write_all(&freq.to_le_bytes()).unwrap();
    }
}

// build_if_missing only downloads when the ngrams file is absent.
// Pre-seeding the binary file lets load() skip the network path entirely.

#[test]
fn load_from_seeded_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_bin(
        &dir,
        Language::English,
        5,
        &[("crane", 300), ("stare", 200), ("light", 100)],
    );

    let ws: WordSet<5> = dir.load(Language::English, None).unwrap();
    assert_eq!(ws.len(), 3);
    assert_eq!(
        ws.frequency(&Word::<5>::try_from("crane").unwrap()),
        Some(300)
    );
    assert_eq!(
        ws.frequency(&Word::<5>::try_from("light").unwrap()),
        Some(100)
    );
}

#[test]
fn load_respects_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    // entries written in freq-desc order so the top-2 are crane and stare
    seed_bin(
        &dir,
        Language::English,
        5,
        &[("crane", 300), ("stare", 200), ("light", 100)],
    );

    let ws: WordSet<5> = dir.load(Language::English, Some(2)).unwrap();
    assert_eq!(ws.len(), 2);
    assert!(ws.contains(&Word::<5>::try_from("crane").unwrap()));
    assert!(ws.contains(&Word::<5>::try_from("stare").unwrap()));
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
