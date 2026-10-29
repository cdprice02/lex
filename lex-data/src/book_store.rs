//! Opening book files: `books/<lang>/<N>/<key-hash>.bin` (ADR 0011).
//!
//! A book is a cache of the solver's policy, so its identity is everything that can change a
//! chosen guess: the [`BookKey`]. The file name is the SHA-256 of the key's canonical rendering,
//! so books for different builds and Strategies coexist.
//!
//! Layout (little-endian), `header_len` bytes of header, then the body:
//!
//! | offset | size    | field                                        |
//! |--------|---------|----------------------------------------------|
//! | 0      | 4       | magic `LEXB`                                 |
//! | 4      | 2       | format version                               |
//! | 6      | 2       | word length `N`                              |
//! | 8      | 4       | `header_len`, offset of the body             |
//! | 12     | 8       | node count                                   |
//! | 20     | 32      | SHA-256 of the body                          |
//! | 52     | 4       | key length                                   |
//! | 56     | 8       | zero padding                                 |
//! | 64     | key len | the [`BookKey`], postcard                    |
//!
//! The body is the [`StoredBook`] in postcard. The checksum covers the body only, like the
//! ngrams cache (ADR 0001), and the key sits in the header so a stale file can be diagnosed
//! without reading its body.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::data_dir::DataDir;
use crate::error::LexDataError;
use crate::language::Language;
use crate::store::hex;

const MAGIC: [u8; 4] = *b"LEXB";
const VERSION: u16 = 1;
const FIXED_LEN: usize = 64;
const KEY_HASH_DOMAIN: &str = "lex-book-key-v1";

/// A Profile as a book key names it: the stored name, plus the checksums of both lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileKey {
    pub name: String,
    pub answers_sha256: String,
    pub guess_only_sha256: String,
}

/// Everything that can change a guess the solver chooses. The Strategy arrives as ordered
/// (field name, value) strings rendered by `lex-core`, which this crate stores and compares
/// without knowing what the fields mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookKey {
    pub corpus_sha256: String,
    /// Absent on a Corpus run.
    pub profile: Option<ProfileKey>,
    pub word_length: usize,
    pub dictionary_length: usize,
    pub strategy: Vec<(String, String)>,
    /// The hash of the solver source the book was built with.
    pub solver_digest: String,
}

impl BookKey {
    /// The key as ordered (name, value) pairs; the unit a stale book is diagnosed in.
    pub fn fields(&self) -> Vec<(String, String)> {
        let none = || "none".to_owned();
        let mut out = vec![
            ("corpus_sha256".to_owned(), self.corpus_sha256.clone()),
            (
                "profile".to_owned(),
                self.profile.as_ref().map_or_else(none, |p| p.name.clone()),
            ),
            (
                "profile_answers_sha256".to_owned(),
                self.profile
                    .as_ref()
                    .map_or_else(none, |p| p.answers_sha256.clone()),
            ),
            (
                "profile_guess_only_sha256".to_owned(),
                self.profile
                    .as_ref()
                    .map_or_else(none, |p| p.guess_only_sha256.clone()),
            ),
            ("word_length".to_owned(), self.word_length.to_string()),
            (
                "dictionary_length".to_owned(),
                self.dictionary_length.to_string(),
            ),
        ];
        out.extend(
            self.strategy
                .iter()
                .map(|(name, value)| (format!("strategy.{name}"), value.clone())),
        );
        out.push(("solver_digest".to_owned(), self.solver_digest.clone()));
        out
    }

    /// Names of the fields on which `self` and `other` differ.
    pub fn differing_fields(&self, other: &Self) -> Vec<String> {
        let (a, b) = (self.fields(), other.fields());
        let mut out: Vec<String> = a
            .iter()
            .filter(|(name, value)| {
                b.iter()
                    .find(|(n, _)| n == name)
                    .is_none_or(|(_, v)| v != value)
            })
            .map(|(name, _)| name.clone())
            .collect();
        out.extend(
            b.iter()
                .filter(|(name, _)| !a.iter().any(|(n, _)| n == name))
                .map(|(name, _)| name.clone()),
        );
        out
    }

    /// Hex SHA-256 of the canonical rendering: the file's name.
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(KEY_HASH_DOMAIN.as_bytes());
        for (name, value) in self.fields() {
            hasher.update(b"\n");
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
        }
        hex(&hasher.finalize())
    }
}

/// One booked position: the pool index of its guess and the booked children by feedback ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredNode {
    pub guess: u32,
    pub edges: Vec<(u64, u32)>,
}

/// The nodes of an Opening book in preorder, with the root first. This crate does not interpret
/// them: `lex-core` validates them against its pool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredBook {
    pub nodes: Vec<StoredNode>,
}

/// A book file as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookFile {
    pub path: PathBuf,
    pub bytes: u64,
}

/// The outcome of looking a key up on disk.
#[derive(Debug, PartialEq, Eq)]
pub enum BookLookup {
    Found(StoredBook),
    /// No file for the key. `nearest` names the fields on which the closest book of the same
    /// language and length differs, or is `None` when there is no book there at all.
    Missing {
        nearest: Option<Vec<String>>,
    },
}

fn corrupt(path: &Path, reason: impl Into<String>) -> LexDataError {
    LexDataError::CorruptBook {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

struct Header {
    word_length: u16,
    header_len: usize,
    node_count: u64,
    checksum: [u8; 32],
    key: BookKey,
}

fn encode(key: &BookKey, book: &StoredBook) -> anyhow::Result<Vec<u8>> {
    let key_bytes = postcard::to_stdvec(key)?;
    let body = postcard::to_stdvec(book)?;
    let header_len = FIXED_LEN + key_bytes.len();
    let mut out = Vec::with_capacity(header_len + body.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(
        &u16::try_from(key.word_length)
            .expect("word length fits in u16")
            .to_le_bytes(),
    );
    out.extend_from_slice(&u32::try_from(header_len)?.to_le_bytes());
    out.extend_from_slice(&(book.nodes.len() as u64).to_le_bytes());
    out.extend_from_slice(&Sha256::digest(&body));
    out.extend_from_slice(&u32::try_from(key_bytes.len())?.to_le_bytes());
    out.resize(FIXED_LEN, 0);
    out.extend_from_slice(&key_bytes);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Parses the header at the start of `bytes`, which must hold at least the fixed part and the key.
fn parse_header(bytes: &[u8], path: &Path) -> Result<Header, LexDataError> {
    if bytes.len() < MAGIC.len() || bytes[0..4] != MAGIC {
        return Err(corrupt(path, "not an opening book (bad magic)"));
    }
    if bytes.len() < FIXED_LEN {
        return Err(corrupt(
            path,
            format!("truncated header ({} bytes, need {FIXED_LEN})", bytes.len()),
        ));
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    if version != VERSION {
        return Err(corrupt(
            path,
            format!("unsupported format version {version} (this build reads {VERSION})"),
        ));
    }
    let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let key_len = u32::from_le_bytes(bytes[52..56].try_into().unwrap()) as usize;
    if header_len != FIXED_LEN + key_len || bytes.len() < header_len {
        return Err(corrupt(path, "truncated or inconsistent header"));
    }
    let key: BookKey = postcard::from_bytes(&bytes[FIXED_LEN..header_len])
        .map_err(|e| corrupt(path, format!("unreadable key: {e}")))?;
    Ok(Header {
        word_length: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
        header_len,
        node_count: u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
        checksum: bytes[20..52].try_into().unwrap(),
        key,
    })
}

fn decode(bytes: &[u8], path: &Path) -> Result<(BookKey, StoredBook), LexDataError> {
    let header = parse_header(bytes, path)?;
    let body = &bytes[header.header_len..];
    if <[u8; 32]>::from(Sha256::digest(body)) != header.checksum {
        return Err(corrupt(path, "body checksum mismatch"));
    }
    let book: StoredBook =
        postcard::from_bytes(body).map_err(|e| corrupt(path, format!("unreadable body: {e}")))?;
    if book.nodes.len() as u64 != header.node_count {
        return Err(corrupt(
            path,
            format!(
                "header says {} nodes, body holds {}",
                header.node_count,
                book.nodes.len()
            ),
        ));
    }
    if usize::from(header.word_length) != header.key.word_length {
        return Err(corrupt(path, "header word length disagrees with its key"));
    }
    Ok((header.key, book))
}

/// Reads just the key at the start of the book file at `path`.
fn read_key(path: &Path) -> anyhow::Result<BookKey> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening opening book: {path:?}"))?;
    let mut fixed = [0u8; FIXED_LEN];
    let got = file.read(&mut fixed)?;
    if got < FIXED_LEN {
        return Err(corrupt(path, "truncated header").into());
    }
    let key_len = u32::from_le_bytes(fixed[52..56].try_into().unwrap()) as usize;
    let mut bytes = fixed.to_vec();
    bytes.resize(FIXED_LEN + key_len, 0);
    file.read_exact(&mut bytes[FIXED_LEN..])
        .map_err(|_| corrupt(path, "truncated header"))?;
    Ok(parse_header(&bytes, path)?.key)
}

fn book_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    files.sort();
    files
}

impl DataDir {
    fn books_root(&self) -> PathBuf {
        self.as_ref().join("books")
    }

    /// Path of the book for `key`: `books/<lang>/<N>/<key-hash>.bin`.
    pub fn book_path(&self, lang: Language, key: &BookKey) -> PathBuf {
        self.books_root()
            .join(lang.lang_code())
            .join(key.word_length.to_string())
            .join(format!("{}.bin", key.hash()))
    }

    /// Reads the book for `key`. A file that is present but fails its checksum, parse, or key
    /// check is an error naming it, never a miss: rebuilding silently would hide the corruption.
    pub fn read_book(&self, lang: Language, key: &BookKey) -> anyhow::Result<BookLookup> {
        let path = self.book_path(lang, key);
        if !path.exists() {
            let nearest = book_files(path.parent().expect("a book path has a parent"))
                .iter()
                .filter_map(|p| read_key(p).ok())
                .map(|k| key.differing_fields(&k))
                .min_by_key(Vec::len);
            return Ok(BookLookup::Missing { nearest });
        }
        let bytes = std::fs::read(&path).with_context(|| format!("reading {path:?}"))?;
        let (stored_key, book) = decode(&bytes, &path)?;
        if &stored_key != key {
            return Err(corrupt(&path, "its key does not match its file name").into());
        }
        Ok(BookLookup::Found(book))
    }

    /// Writes the book for `key` through a temporary file and a rename, so an interrupted build
    /// leaves nothing and a reader sees either no book or a whole one.
    pub fn write_book(
        &self,
        lang: Language,
        key: &BookKey,
        book: &StoredBook,
    ) -> anyhow::Result<BookFile> {
        let path = self.book_path(lang, key);
        let dir = path.parent().expect("a book path has a parent");
        std::fs::create_dir_all(dir)?;
        let bytes = encode(key, book)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&bytes)?;
        tmp.persist(&path)
            .with_context(|| format!("renaming the opening book into {path:?}"))?;
        Ok(BookFile {
            path,
            bytes: bytes.len() as u64,
        })
    }

    /// Removes `books/`, narrowed to a language and a word length. Missing directories are fine.
    pub fn clear_books(&self, lang: Option<Language>, n: Option<usize>) -> anyhow::Result<()> {
        let root = self.books_root();
        let langs: Vec<PathBuf> = match lang {
            Some(lang) => vec![root.join(lang.lang_code())],
            None => std::fs::read_dir(&root)
                .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
                .unwrap_or_default(),
        };
        for lang_dir in langs {
            let target = match n {
                Some(n) => lang_dir.join(n.to_string()),
                None => lang_dir,
            };
            match std::fs::remove_dir_all(&target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("removing {target:?}")),
            }
        }
        Ok(())
    }

    /// Removes the books whose key names the Profile `name`.
    pub(crate) fn clear_profile_books(&self, name: &str) -> anyhow::Result<()> {
        let Ok(langs) = std::fs::read_dir(self.books_root()) else {
            return Ok(());
        };
        for lang_dir in langs.filter_map(|e| e.ok()) {
            let Ok(lengths) = std::fs::read_dir(lang_dir.path()) else {
                continue;
            };
            for length_dir in lengths.filter_map(|e| e.ok()) {
                for file in book_files(&length_dir.path()) {
                    match read_key(&file) {
                        Ok(key) if key.profile.as_ref().is_some_and(|p| p.name == name) => {
                            std::fs::remove_file(&file)
                                .with_context(|| format!("removing {file:?}"))?;
                        }
                        Ok(_) => {}
                        Err(e) => log::warn!("skipping unreadable book {file:?}: {e:#}"),
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> BookKey {
        BookKey {
            corpus_sha256: "c0".to_owned(),
            profile: Some(ProfileKey {
                name: "wordle".to_owned(),
                answers_sha256: "a0".to_owned(),
                guess_only_sha256: "g0".to_owned(),
            }),
            word_length: 5,
            dictionary_length: 100,
            strategy: vec![
                ("pool".to_owned(), "all".to_owned()),
                ("temperature".to_owned(), "5".to_owned()),
            ],
            solver_digest: "d0".to_owned(),
        }
    }

    fn book() -> StoredBook {
        StoredBook {
            nodes: vec![
                StoredNode {
                    guess: 3,
                    edges: vec![(1, 1), (40, 2)],
                },
                StoredNode {
                    guess: 7,
                    edges: vec![],
                },
                StoredNode {
                    guess: 9,
                    edges: vec![],
                },
            ],
        }
    }

    fn dir() -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    const LANG: Language = Language::English;

    #[test]
    fn a_written_book_reads_back_equal_at_its_keyed_path() {
        let (_tmp, dir) = dir();
        let file = dir.write_book(LANG, &key(), &book()).unwrap();
        assert_eq!(file.path, dir.book_path(LANG, &key()), "{:?}", file.path);
        assert!(
            file.path
                .ends_with(format!("books/eng/5/{}.bin", key().hash()))
        );
        assert_eq!(file.bytes, std::fs::metadata(&file.path).unwrap().len());
        assert_eq!(
            dir.read_book(LANG, &key()).unwrap(),
            BookLookup::Found(book())
        );
    }

    #[test]
    fn a_missing_book_with_no_siblings_has_no_nearest() {
        let (_tmp, dir) = dir();
        assert_eq!(
            dir.read_book(LANG, &key()).unwrap(),
            BookLookup::Missing { nearest: None }
        );
    }

    #[test]
    fn changing_any_key_field_misses_and_names_that_field() {
        let (_tmp, dir) = dir();
        dir.write_book(LANG, &key(), &book()).unwrap();
        type Edit = Box<dyn Fn(&mut BookKey)>;
        let edits: Vec<(&str, Edit)> = vec![
            ("corpus_sha256", Box::new(|k| k.corpus_sha256 = "c1".into())),
            (
                "profile",
                Box::new(|k| k.profile.as_mut().unwrap().name = "x".into()),
            ),
            (
                "profile_answers_sha256",
                Box::new(|k| k.profile.as_mut().unwrap().answers_sha256 = "a1".into()),
            ),
            (
                "profile_guess_only_sha256",
                Box::new(|k| k.profile.as_mut().unwrap().guess_only_sha256 = "g1".into()),
            ),
            ("word_length", Box::new(|k| k.word_length = 6)),
            ("dictionary_length", Box::new(|k| k.dictionary_length = 99)),
            (
                "strategy.pool",
                Box::new(|k| k.strategy[0].1 = "hard".into()),
            ),
            (
                "strategy.temperature",
                Box::new(|k| k.strategy[1].1 = "4".into()),
            ),
            ("solver_digest", Box::new(|k| k.solver_digest = "d1".into())),
        ];
        for (field, edit) in edits {
            let mut changed = key();
            edit(&mut changed);
            if field == "word_length" {
                // A different length lives in a different directory.
                assert_eq!(changed.differing_fields(&key()), [field]);
                continue;
            }
            assert_eq!(
                dir.read_book(LANG, &changed).unwrap(),
                BookLookup::Missing {
                    nearest: Some(vec![field.to_owned()])
                },
                "{field}"
            );
        }
    }

    #[test]
    fn a_dropped_profile_differs_in_all_three_profile_fields() {
        let mut corpus = key();
        corpus.profile = None;
        assert_eq!(
            corpus.differing_fields(&key()),
            [
                "profile",
                "profile_answers_sha256",
                "profile_guess_only_sha256"
            ]
        );
    }

    #[test]
    fn the_nearest_book_is_the_one_with_fewest_differences() {
        let (_tmp, dir) = dir();
        let mut far = key();
        far.solver_digest = "d1".into();
        far.dictionary_length = 1;
        let mut near = key();
        near.solver_digest = "d2".into();
        dir.write_book(LANG, &far, &book()).unwrap();
        dir.write_book(LANG, &near, &book()).unwrap();
        assert_eq!(
            dir.read_book(LANG, &key()).unwrap(),
            BookLookup::Missing {
                nearest: Some(vec!["solver_digest".to_owned()])
            }
        );
    }

    #[test]
    fn distinct_keys_hash_to_distinct_files() {
        let mut other = key();
        other.strategy[1].1 = "4".into();
        assert_ne!(key().hash(), other.hash());
        assert_eq!(key().hash().len(), 64);
    }

    fn corrupt_at(dir: &DataDir, edit: impl FnOnce(&mut Vec<u8>)) -> String {
        let path = dir.write_book(LANG, &key(), &book()).unwrap().path;
        let mut bytes = std::fs::read(&path).unwrap();
        edit(&mut bytes);
        std::fs::write(&path, bytes).unwrap();
        let err = dir.read_book(LANG, &key()).unwrap_err().to_string();
        assert!(err.contains(path.to_str().unwrap()), "{err}");
        assert!(err.contains("lex book build --force"), "{err}");
        err
    }

    #[test]
    fn a_flipped_body_byte_fails_the_checksum() {
        let (_tmp, dir) = dir();
        let err = corrupt_at(&dir, |b| *b.last_mut().unwrap() ^= 1);
        assert!(err.contains("checksum"), "{err}");
    }

    #[test]
    fn a_truncated_file_is_corrupt() {
        let (_tmp, dir) = dir();
        corrupt_at(&dir, |b| b.truncate(10));
        let (_tmp, dir) = self::dir();
        corrupt_at(&dir, |b| b.truncate(b.len() - 2));
    }

    #[test]
    fn a_foreign_file_and_an_unknown_version_are_corrupt() {
        let (_tmp, dir) = dir();
        let err = corrupt_at(&dir, |b| b[0] = b'X');
        assert!(err.contains("magic"), "{err}");
        let (_tmp, dir) = self::dir();
        let err = corrupt_at(&dir, |b| b[4] = 99);
        assert!(err.contains("version 99"), "{err}");
    }

    #[test]
    fn a_book_renamed_to_another_keys_name_is_corrupt() {
        let (_tmp, dir) = dir();
        let mut other = key();
        other.solver_digest = "d1".into();
        let written = dir.write_book(LANG, &other, &book()).unwrap().path;
        std::fs::rename(&written, dir.book_path(LANG, &key())).unwrap();
        let err = dir.read_book(LANG, &key()).unwrap_err().to_string();
        assert!(err.contains("key does not match"), "{err}");
    }

    #[test]
    fn writing_leaves_no_temporary_file_and_replaces_in_place() {
        let (_tmp, dir) = dir();
        let first = dir.write_book(LANG, &key(), &book()).unwrap();
        let again = dir
            .write_book(LANG, &key(), &StoredBook::default())
            .unwrap();
        assert_eq!(first.path, again.path);
        assert_eq!(
            dir.read_book(LANG, &key()).unwrap(),
            BookLookup::Found(StoredBook::default())
        );
        let entries = std::fs::read_dir(first.path.parent().unwrap())
            .unwrap()
            .count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn clear_books_narrows_by_language_and_length() {
        let (_tmp, dir) = dir();
        let mut six = key();
        six.word_length = 6;
        dir.write_book(LANG, &key(), &book()).unwrap();
        dir.write_book(LANG, &six, &book()).unwrap();
        dir.write_book(Language::French, &key(), &book()).unwrap();

        dir.clear_books(Some(LANG), Some(6)).unwrap();
        assert!(dir.book_path(LANG, &key()).exists());
        assert!(!dir.book_path(LANG, &six).exists());

        dir.clear_books(Some(LANG), None).unwrap();
        assert!(!dir.book_path(LANG, &key()).exists());
        assert!(dir.book_path(Language::French, &key()).exists());

        dir.clear_books(None, None).unwrap();
        assert!(!dir.book_path(Language::French, &key()).exists());
        dir.clear_books(None, None).unwrap();
    }

    #[test]
    fn clear_books_never_touches_other_caches() {
        let (tmp, dir) = dir();
        dir.write_book(LANG, &key(), &book()).unwrap();
        let ngrams = tmp.path().join("ngrams/eng/5.bin");
        std::fs::create_dir_all(ngrams.parent().unwrap()).unwrap();
        std::fs::write(&ngrams, b"x").unwrap();
        dir.clear_books(None, None).unwrap();
        assert!(ngrams.exists());
    }

    #[test]
    fn clear_profile_books_removes_only_that_profiles_books() {
        let (_tmp, dir) = dir();
        let mut other_profile = key();
        other_profile.profile.as_mut().unwrap().name = "wordle-all".into();
        let mut corpus = key();
        corpus.profile = None;
        for k in [&key(), &other_profile, &corpus] {
            dir.write_book(LANG, k, &book()).unwrap();
        }
        dir.clear_profile_books("wordle").unwrap();
        assert!(!dir.book_path(LANG, &key()).exists());
        assert!(dir.book_path(LANG, &other_profile).exists());
        assert!(dir.book_path(LANG, &corpus).exists());
    }
}
