#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]
#![expect(
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

//! The one test that spells the Corpus file layout out byte by byte. Every other fixture goes
//! through `lex_data::testing`, which uses the real writers, so this is the independent oracle:
//! if the writer and the reader drift together, this still fails.

use lex_data::testing::seed_corpus;
use lex_data::{DataDir, Language, Word};
use sha2::{Digest, Sha256};

/// The layout of ADR 0001, by hand: a 64-byte little-endian header (magic `LEXC`, version 1,
/// word length, header length 64, record count, SHA-256 of the records, zero padding), then per
/// record the word's letters as `u32` scalars and a `u64` frequency.
fn hand_written_corpus(words: &[(&str, u64)]) -> Vec<u8> {
    let mut records = Vec::new();
    for &(word, freq) in words {
        for ch in word.chars() {
            records.extend_from_slice(&u32::from(ch).to_le_bytes());
        }
        records.extend_from_slice(&freq.to_le_bytes());
    }
    let mut file = Vec::new();
    file.extend_from_slice(b"LEXC");
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&5u16.to_le_bytes());
    file.extend_from_slice(&64u32.to_le_bytes());
    file.extend_from_slice(&u64::try_from(words.len()).unwrap().to_le_bytes());
    file.extend_from_slice(&Sha256::digest(&records));
    file.resize(64, 0);
    file.extend_from_slice(&records);
    file
}

#[test]
fn the_writer_emits_the_documented_layout_and_the_reader_accepts_it() {
    let words = [("crane", 300), ("stare", 200), ("light", 200)];
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_corpus(&dir, Language::English, 5, &words);
    let path = dir.ngrams_path(Language::English, 5);
    assert_eq!(std::fs::read(&path).unwrap(), hand_written_corpus(&words));

    let by_hand = tempfile::tempdir().unwrap();
    let other = DataDir::new(by_hand.path());
    let path = other.ngrams_path(Language::English, 5);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, hand_written_corpus(&words)).unwrap();
    let set = other
        .cached_corpus::<5>(Language::English)
        .unwrap()
        .unwrap();
    assert_eq!(set.len(), 3);
    assert_eq!(
        set.word_frequencies().next(),
        Some((Word::<5>::try_from("crane").unwrap(), 300))
    );
}
