//! Fixtures shared by the integration tests.

use std::fs;
use std::io::Write;

use lex_data::{DataDir, Language};

/// Writes a cache file in the documented on-disk format, so `load` never touches the network.
pub(crate) fn seed_bin(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
    use sha2::{Digest, Sha256};
    let path = dir.ngrams_path(lang, n);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut records = Vec::new();
    for &(word, freq) in entries {
        for ch in word.chars() {
            records.extend_from_slice(&(ch as u32).to_le_bytes());
        }
        records.extend_from_slice(&freq.to_le_bytes());
    }
    let mut header = [0u8; 64];
    header[0..4].copy_from_slice(b"LEXC");
    header[4..6].copy_from_slice(&1u16.to_le_bytes());
    header[6..8].copy_from_slice(&(n as u16).to_le_bytes());
    header[8..12].copy_from_slice(&64u32.to_le_bytes());
    header[12..20].copy_from_slice(&(entries.len() as u64).to_le_bytes());
    header[20..52].copy_from_slice(&Sha256::digest(&records));
    let mut f = fs::File::create(path).unwrap();
    f.write_all(&header).unwrap();
    f.write_all(&records).unwrap();
}

/// Stands in for the cached Wiktionary list, so the report's provenance lookup never downloads.
pub(crate) fn seed_dict(dir: &DataDir, lang: Language) {
    let path = dir.dict_path(lang);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "").unwrap();
    fs::write(
        path.with_extension("meta.json"),
        r#"{"min_word_length":5,"max_word_length":5,"word_count":8}"#,
    )
    .unwrap();
}
