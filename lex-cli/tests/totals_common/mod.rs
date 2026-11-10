//! Fixture for the tests that need Ngrams totals on disk.
#![expect(
    clippy::unwrap_used,
    reason = "`clippy.toml` exempts `#[test]` functions and `#[cfg(test)]` modules, not the plain helpers of an integration test, where a failed fixture should abort the run"
)]

use std::fs;
use std::io::Write;

use lex_data::{DataDir, Language};

/// Writes a complete set of Ngrams totals in the documented format: a `LEXT` file per length in
/// the Corpus layout and a `meta.json` last, so `DataDir::totals` reads it as complete.
pub fn seed_totals(dir: &DataDir, lang: Language, lengths: &[(usize, &[(&str, u64)])]) {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut files = String::new();
    for &(n, entries) in lengths {
        let path = dir.totals_path(lang, n);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut records = Vec::new();
        for &(word, freq) in entries {
            for ch in word.chars() {
                records.extend_from_slice(&u32::from(ch).to_le_bytes());
            }
            records.extend_from_slice(&freq.to_le_bytes());
        }
        let sum = Sha256::digest(&records);
        let mut header = [0u8; 64];
        header[0..4].copy_from_slice(b"LEXT");
        header[4..6].copy_from_slice(&1u16.to_le_bytes());
        header[6..8].copy_from_slice(&u16::try_from(n).unwrap().to_le_bytes());
        header[8..12].copy_from_slice(&64u32.to_le_bytes());
        header[12..20].copy_from_slice(&u64::try_from(entries.len()).unwrap().to_le_bytes());
        header[20..52].copy_from_slice(&sum);
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&header).unwrap();
        f.write_all(&records).unwrap();
        let hex: String = sum.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
        if !files.is_empty() {
            files.push(',');
        }
        let _ = write!(
            files,
            r#""{n}":{{"records":{},"sha256":"{hex}"}}"#,
            entries.len()
        );
    }
    fs::write(
        dir.totals_dir(lang).join("meta.json"),
        format!(
            r#"{{"snapshot":"{}","shard_count":{},"files":{{{files}}}}}"#,
            lex_data::NGRAMS_SNAPSHOT,
            lang.shard_count()
        ),
    )
    .unwrap();
}
