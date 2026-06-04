use std::collections::HashMap;

use lex_data::{Language, Word, WordSet};

fn toy_english_5() -> HashMap<usize, HashMap<String, u64>> {
    let mut five = HashMap::new();
    five.insert("crane".to_string(), 300u64);
    five.insert("stare".to_string(), 200);
    five.insert("light".to_string(), 100);
    five.insert("mount".to_string(), 75);
    five.insert("swipe".to_string(), 25);
    let mut by_length = HashMap::new();
    by_length.insert(5, five);
    by_length
}

#[test]
fn cache_path_structure() {
    let base = std::path::Path::new("/tmp/lex_test");
    let path = lex_data::cache_path(base, Language::English, 5);
    assert_eq!(path, base.join("eng").join("5.csv"));
}

#[test]
fn put_read_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let lang = Language::English;

    lex_data::put(dir.path(), lang, &toy_english_5()).unwrap();

    let ws: WordSet<5> = lex_data::blocking::get(dir.path(), lang, None).unwrap();
    assert_eq!(ws.len(), 5);
    assert_eq!(
        ws.frequency(&Word::<5>::try_from("crane").unwrap()),
        Some(300)
    );
    assert_eq!(
        ws.frequency(&Word::<5>::try_from("swipe").unwrap()),
        Some(25)
    );
}

#[test]
fn read_respects_max() {
    let dir = tempfile::tempdir().unwrap();
    let lang = Language::English;

    lex_data::put(dir.path(), lang, &toy_english_5()).unwrap();

    let ws: WordSet<5> = lex_data::blocking::get(dir.path(), lang, Some(2)).unwrap();
    assert_eq!(ws.len(), 2);
    // put() writes sorted descending by freq: crane (300) and stare (200) are top-2
    assert!(ws.contains(&Word::<5>::try_from("crane").unwrap()));
    assert!(ws.contains(&Word::<5>::try_from("stare").unwrap()));
}

#[test]
fn invalidate_file() {
    let dir = tempfile::tempdir().unwrap();
    let lang = Language::English;

    let mut by_length = toy_english_5();
    let mut three = HashMap::new();
    three.insert("ace".to_string(), 100u64);
    by_length.insert(3, three);

    lex_data::put(dir.path(), lang, &by_length).unwrap();

    let five_path = lex_data::cache_path(dir.path(), lang, 5);
    let three_path = lex_data::cache_path(dir.path(), lang, 3);
    assert!(five_path.exists());
    assert!(three_path.exists());

    lex_data::invalidate(dir.path(), lang, Some(5)).unwrap();
    assert!(!five_path.exists());
    assert!(three_path.exists());
}

#[test]
fn invalidate_dir() {
    let dir = tempfile::tempdir().unwrap();
    let lang = Language::English;

    lex_data::put(dir.path(), lang, &toy_english_5()).unwrap();
    let lang_dir = dir.path().join("eng");
    assert!(lang_dir.exists());

    lex_data::invalidate(dir.path(), lang, None).unwrap();
    assert!(!lang_dir.exists());
}
