//! Fixed-size header at the start of every ngrams cache file.
//!
//! Layout (little-endian), `header_len` bytes in total:
//!
//! | offset | size | field                                        |
//! |--------|------|----------------------------------------------|
//! | 0      | 4    | magic `LEXC`                                 |
//! | 4      | 2    | format version                               |
//! | 6      | 2    | word length `N`                              |
//! | 8      | 4    | `header_len`, offset of the first record     |
//! | 12     | 8    | record count                                 |
//! | 20     | 32   | SHA-256 of the records region                |
//! | 52     | 12   | zero padding                                 |
//!
//! `header_len` lets a later version append fields without moving the parser's
//! assumptions: records always start at that offset.

use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};

use crate::error::LexDataError;

/// File magic identifying an ngrams Cache.
pub const MAGIC: [u8; 4] = *b"LEXC";
/// Format version of the Cache header.
pub const VERSION: u16 = 1;
/// Bytes of header this version writes.
pub const HEADER_LEN: usize = 64;

/// The parsed header of a Cache file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Word length `N` the Cache holds.
    pub(crate) word_len: u16,
    /// Offset of the first record.
    pub(crate) records_offset: u32,
    /// Number of records.
    pub(crate) record_count: u64,
    /// SHA-256 of the records region.
    pub(crate) checksum: [u8; 32],
}

impl Header {
    /// Header for a records region: counts `records.len() / record_size` records and
    /// hashes the bytes.
    pub(crate) fn for_records(word_len: usize, record_size: usize, records: &[u8]) -> Self {
        Self {
            word_len: u16::try_from(word_len).expect("word length fits in u16"),
            records_offset: HEADER_LEN as u32,
            record_count: (records.len() / record_size) as u64,
            checksum: Sha256::digest(records).into(),
        }
    }

    /// The header as `HEADER_LEN` little-endian bytes.
    pub(crate) fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&self.word_len.to_le_bytes());
        out[8..12].copy_from_slice(&self.records_offset.to_le_bytes());
        out[12..20].copy_from_slice(&self.record_count.to_le_bytes());
        out[20..52].copy_from_slice(&self.checksum);
        out
    }

    /// Parses the header at the start of `bytes`. A file without the magic is a legacy
    /// cache; any other malformation is a corrupt one.
    pub(crate) fn parse(bytes: &[u8], path: &Path) -> Result<Self, LexDataError> {
        let corrupt = |reason: String| LexDataError::CorruptCache {
            path: path.to_path_buf(),
            reason,
        };
        if bytes.len() < MAGIC.len() || bytes[0..4] != MAGIC {
            return Err(LexDataError::LegacyCache {
                path: path.to_path_buf(),
            });
        }
        if bytes.len() < HEADER_LEN {
            return Err(corrupt(format!(
                "truncated header ({} bytes, need {HEADER_LEN})",
                bytes.len()
            )));
        }
        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version != VERSION {
            return Err(corrupt(format!(
                "unsupported format version {version} (this build reads {VERSION})"
            )));
        }
        let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if (header_len as usize) < HEADER_LEN {
            return Err(corrupt(format!("header length {header_len} is too small")));
        }
        Ok(Self {
            word_len: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            records_offset: header_len,
            record_count: u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
            checksum: bytes[20..52].try_into().unwrap(),
        })
    }

    /// Reads just the header of the cache file at `path`.
    pub(crate) fn read_file(path: &Path) -> anyhow::Result<Self> {
        use std::io::Read;
        let mut buf = Vec::with_capacity(HEADER_LEN);
        std::fs::File::open(path)
            .with_context(|| format!("opening ngrams file: {}", path.display()))?
            .take(HEADER_LEN as u64)
            .read_to_end(&mut buf)
            .with_context(|| format!("reading ngrams header: {}", path.display()))?;
        Ok(Self::parse(&buf, path)?)
    }

    /// The records checksum as lowercase hex: the Corpus checksum.
    pub(crate) fn checksum_hex(&self) -> String {
        crate::store::hex(&self.checksum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        Header::for_records(5, 28, &[7u8; 28 * 3])
    }

    #[test]
    fn roundtrip() {
        let h = sample();
        assert_eq!(h.record_count, 3);
        assert_eq!(Header::parse(&h.encode(), Path::new("x")).unwrap(), h);
    }

    #[test]
    fn missing_magic_is_legacy() {
        let err = Header::parse(&[0u8; 100], Path::new("x")).unwrap_err();
        assert!(matches!(err, LexDataError::LegacyCache { .. }));
        let err = Header::parse(&[], Path::new("x")).unwrap_err();
        assert!(matches!(err, LexDataError::LegacyCache { .. }));
    }

    #[test]
    fn unsupported_version_is_corrupt() {
        let mut bytes = sample().encode();
        bytes[4] = 99;
        let err = Header::parse(&bytes, Path::new("x")).unwrap_err();
        assert!(err.to_string().contains("version 99"), "{err}");
    }

    #[test]
    fn truncated_header_is_corrupt() {
        let bytes = sample().encode();
        let err = Header::parse(&bytes[..30], Path::new("x")).unwrap_err();
        assert!(err.to_string().contains("truncated header"), "{err}");
    }
}
