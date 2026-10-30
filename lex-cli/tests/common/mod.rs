//! Fixtures shared by the integration tests.
#![expect(
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::fs;
use std::io::Write;

use lex_data::{DataDir, Language};

/// Writes a cache file in the documented on-disk format, so `load` never touches the network.
pub fn seed_bin(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
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

/// Seeds the cache with `head` followed by distinct low-frequency five-letter words, up to the
/// 200 a length needs to be Supported. The head keeps its frequencies and its order.
pub fn seed_supported(dir: &DataDir, lang: Language, n: usize, head: &[(&str, u64)]) {
    let floor = head.iter().map(|&(_, f)| f).min().unwrap_or(1);
    let padding: Vec<(String, u64)> = (0..lex_data::MIN_SUPPORTED_WORDS.saturating_sub(head.len()))
        .map(|i| {
            let letters: String = [i / 676, i / 26 % 26, i % 26]
                .iter()
                .map(|&d| char::from(b'a' + u8::try_from(d).unwrap()))
                .collect();
            (
                format!("zz{letters}"),
                floor.saturating_sub(1 + u64::try_from(i).unwrap()).max(1),
            )
        })
        .collect();
    let entries: Vec<(&str, u64)> = head
        .iter()
        .copied()
        .chain(padding.iter().map(|(w, f)| (w.as_str(), *f)))
        .collect();
    seed_bin(dir, lang, n, &entries);
}

/// Stands in for the cached Wiktionary list, so the report's provenance lookup never downloads.
pub fn seed_dict(dir: &DataDir, lang: Language) {
    let path = dir.dict_path(lang);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "").unwrap();
    fs::write(
        path.with_extension("meta.json"),
        r#"{"min_word_length":5,"max_word_length":5,"word_count":8}"#,
    )
    .unwrap();
}
