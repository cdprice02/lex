//! Fixed-size header at the start of every ngrams cache file.
//!
//! Layout (little-endian), `header_len` bytes in total:
//!
//! | offset | size | field                                        |
//! |--------|------|----------------------------------------------|
//! | 0      | 4    | magic `LEXC` (Corpus) or `LEXT` (Ngrams totals) |
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

use crate::le::array_at;

use anyhow::Context;
use sha2::{Digest, Sha256};

use crate::error::LexDataError;

/// Size in bytes of one record of a file of word length `n`: `n` letters as `u32` scalars, then
/// a `u64` frequency.
pub const fn record_size(n: usize) -> usize {
    n * std::mem::size_of::<u32>() + std::mem::size_of::<u64>()
}

/// File magic identifying a Corpus Cache.
pub const MAGIC: [u8; 4] = *b"LEXC";
/// File magic identifying an Ngrams totals file, which has the Corpus layout otherwise.
pub const TOTALS_MAGIC: [u8; 4] = *b"LEXT";
/// Format version of the Cache header.
pub const VERSION: u16 = 1;
/// Bytes of header this version writes.
pub const HEADER_LEN: usize = 64;

/// Which kind of file a header introduces: both share one layout and differ in the magic, so
/// each reader rejects the other's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A Corpus Cache, `LEXC`.
    Corpus,
    /// An Ngrams totals file, `LEXT`.
    Totals,
}

impl Kind {
    /// The file magic of this kind.
    pub const fn magic(self) -> [u8; 4] {
        match self {
            Self::Corpus => MAGIC,
            Self::Totals => TOTALS_MAGIC,
        }
    }

    /// The other kind.
    const fn other(self) -> Self {
        match self {
            Self::Corpus => Self::Totals,
            Self::Totals => Self::Corpus,
        }
    }

    /// What the kind is called in a message.
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Corpus => "Corpus cache",
            Self::Totals => "Ngrams totals file",
        }
    }
}

/// How a Cache file's header reads, for listing without loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inspected {
    /// A header of this build's format version.
    Header(Header),
    /// No magic: a Legacy cache.
    Legacy,
    /// The magic with another format version.
    OldVersion(u16),
    /// The magic but a truncated or malformed header.
    Unreadable,
}

/// The parsed header of a Cache file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Which kind of file this header introduces.
    pub(crate) kind: Kind,
    /// Word length `N` the Cache holds.
    pub(crate) word_len: u16,
    /// Offset of the first record.
    pub(crate) records_offset: usize,
    /// Number of records.
    pub(crate) record_count: usize,
    /// SHA-256 of the records region.
    pub(crate) checksum: [u8; 32],
}

impl Header {
    /// Header for a records region: counts `records.len() / record_size` records and
    /// hashes the bytes.
    pub(crate) fn for_records(
        word_len: usize,
        record_size: usize,
        records: &[u8],
    ) -> Result<Self, std::num::TryFromIntError> {
        Self::for_kind(Kind::Corpus, word_len, record_size, records)
    }

    /// [`Self::for_records`] for a file of `kind`.
    pub(crate) fn for_kind(
        kind: Kind,
        word_len: usize,
        record_size: usize,
        records: &[u8],
    ) -> Result<Self, std::num::TryFromIntError> {
        Ok(Self {
            kind,
            word_len: u16::try_from(word_len)?,
            records_offset: HEADER_LEN,
            record_count: records.len() / record_size,
            checksum: Sha256::digest(records).into(),
        })
    }

    /// The header as `HEADER_LEN` little-endian bytes.
    pub(crate) fn encode(&self) -> Result<[u8; HEADER_LEN], std::num::TryFromIntError> {
        let mut out = [0u8; HEADER_LEN];
        out[0..4].copy_from_slice(&self.kind.magic());
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&self.word_len.to_le_bytes());
        out[8..12].copy_from_slice(&u32::try_from(self.records_offset)?.to_le_bytes());
        out[12..20].copy_from_slice(&u64::try_from(self.record_count)?.to_le_bytes());
        out[20..52].copy_from_slice(&self.checksum);
        Ok(out)
    }

    /// Parses the header at the start of `bytes`. A file without the magic is a legacy
    /// cache; any other malformation is a corrupt one.
    pub(crate) fn parse(bytes: &[u8], path: &Path) -> Result<Self, LexDataError> {
        Self::parse_as(bytes, path, Kind::Corpus)
    }

    /// [`Self::parse`] for a file that must be of `kind`; the other kind's magic is a
    /// `WrongCacheKind`.
    pub(crate) fn parse_as(bytes: &[u8], path: &Path, kind: Kind) -> Result<Self, LexDataError> {
        let corrupt = |reason: String| LexDataError::CorruptCache {
            path: path.to_path_buf(),
            reason,
        };
        if bytes.len() >= 4 && bytes[0..4] == kind.other().magic() {
            return Err(LexDataError::WrongCacheKind {
                path: path.to_path_buf(),
                found: kind.other().noun(),
                expected: kind.noun(),
            });
        }
        if bytes.len() < 4 || bytes[0..4] != kind.magic() {
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
        let version = u16::from_le_bytes(array_at(bytes, 4));
        if version != VERSION {
            return Err(corrupt(format!(
                "unsupported format version {version} (this build reads {VERSION})"
            )));
        }
        let header_len = u32::from_le_bytes(array_at(bytes, 8));
        let records_offset = usize::try_from(header_len)
            .ok()
            .filter(|&len| len >= HEADER_LEN)
            .ok_or_else(|| corrupt(format!("header length {header_len} is too small")))?;
        let record_count = u64::from_le_bytes(array_at(bytes, 12));
        let record_count = usize::try_from(record_count)
            .map_err(|_| corrupt(format!("record count {record_count} is too large")))?;
        Ok(Self {
            kind,
            word_len: u16::from_le_bytes(array_at(bytes, 6)),
            records_offset,
            record_count,
            checksum: array_at(bytes, 20),
        })
    }

    /// Reads just the header of the cache file at `path`.
    pub(crate) fn read_file(path: &Path) -> anyhow::Result<Self> {
        Self::read_file_as(path, Kind::Corpus)
    }

    /// Reads just the header of the file at `path`, which must be of `kind`.
    pub(crate) fn read_file_as(path: &Path, kind: Kind) -> anyhow::Result<Self> {
        Ok(Self::parse_as(&read_prefix(path)?, path, kind)?)
    }

    /// How the file at `path` reads in its first `HEADER_LEN` bytes, classified without
    /// reading the records. A bad file is a classification; only an unopenable one errors.
    pub(crate) fn inspect(path: &Path) -> anyhow::Result<Inspected> {
        let buf = read_prefix(path)?;
        if buf.len() < MAGIC.len() || buf[0..4] != MAGIC {
            return Ok(Inspected::Legacy);
        }
        if buf.len() >= 6 {
            let version = u16::from_le_bytes(array_at(&buf, 4));
            if version != VERSION {
                return Ok(Inspected::OldVersion(version));
            }
        }
        Ok(Self::parse(&buf, path).map_or(Inspected::Unreadable, Inspected::Header))
    }

    /// The records checksum as lowercase hex: the Corpus checksum.
    pub(crate) fn checksum_hex(&self) -> String {
        crate::header::hex(&self.checksum)
    }
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// The first `HEADER_LEN` bytes of the file at `path`, or fewer when it is shorter.
fn read_prefix(path: &Path) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::with_capacity(HEADER_LEN);
    std::fs::File::open(path)
        .with_context(|| format!("opening ngrams file: {}", path.display()))?
        .take(u64::try_from(HEADER_LEN)?)
        .read_to_end(&mut buf)
        .with_context(|| format!("reading ngrams header: {}", path.display()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        Header::for_records(5, 28, &[7u8; 28 * 3]).unwrap()
    }

    #[test]
    fn roundtrip() {
        let h = sample();
        assert_eq!(h.record_count, 3);
        assert_eq!(
            Header::parse(&h.encode().unwrap(), Path::new("x")).unwrap(),
            h
        );
    }

    #[test]
    fn each_reader_rejects_the_other_kinds_file() {
        let totals = Header::for_kind(Kind::Totals, 5, 28, &[7u8; 28]).unwrap();
        let bytes = totals.encode().unwrap();
        assert_eq!(&bytes[0..4], b"LEXT");
        assert_eq!(
            Header::parse_as(&bytes, Path::new("x"), Kind::Totals).unwrap(),
            totals
        );
        let err = Header::parse(&bytes, Path::new("x")).unwrap_err();
        assert!(matches!(err, LexDataError::WrongCacheKind { .. }), "{err}");
        let corpus = sample().encode().unwrap();
        let err = Header::parse_as(&corpus, Path::new("x"), Kind::Totals).unwrap_err();
        assert!(matches!(err, LexDataError::WrongCacheKind { .. }), "{err}");
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
        let mut bytes = sample().encode().unwrap();
        bytes[4] = 99;
        let err = Header::parse(&bytes, Path::new("x")).unwrap_err();
        assert!(err.to_string().contains("version 99"), "{err}");
    }

    #[test]
    fn truncated_header_is_corrupt() {
        let bytes = sample().encode().unwrap();
        let err = Header::parse(&bytes[..30], Path::new("x")).unwrap_err();
        assert!(err.to_string().contains("truncated header"), "{err}");
    }
}
