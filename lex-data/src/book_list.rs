//! Reading the stored opening books from disk, never building one, as plain records for any
//! frontend. A record reads a file's header and key and verifies its body checksum.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use strum::VariantArray;

use crate::book_store::{self, BookKey};
use crate::data_dir::DataDir;
use crate::error::LexDataError;
use crate::language::Language;
use crate::profile::Profile;

/// One key field on which a stored book no longer matches what a run would use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleField {
    /// The key field's name, as [`BookKey::fields`] spells it.
    pub name: String,
    /// The value stored in the book.
    pub book: String,
    /// The value a run would use now.
    pub current: String,
}

/// Whether a run of the book's game and Strategy would read the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookStatus {
    /// Its solver digest, Corpus checksum and Profile checksums are the current ones.
    Current,
    /// At least one of them is not. A different Strategy never makes a book stale: it is a
    /// different book.
    Stale(Vec<StaleField>),
    /// The file's header or key is unreadable or contradicts its name; the reason follows.
    Corrupt(String),
}

/// The part of a book file that is read without its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookHeader {
    /// Format version the file declares.
    pub format_version: u16,
    /// Booked positions (nodes in the body).
    pub positions: usize,
    /// The key the book was built under.
    pub key: BookKey,
}

/// A stored opening book as found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRecord {
    /// The language directory the file sits in.
    pub language: Language,
    /// The word length directory the file sits in.
    pub word_length: usize,
    /// The book file.
    pub path: PathBuf,
    /// Size of the file in bytes.
    pub size_bytes: u64,
    /// The file stem: the hash of the key it was built under.
    pub id: String,
    /// The header, `None` when it could not be read.
    pub header: Option<BookHeader>,
    /// Whether a run would read it.
    pub status: BookStatus,
}

/// Number of leading characters of the id that `list` shows.
const SHORT_ID_LEN: usize = 8;

impl BookRecord {
    /// The first 8 characters of the id.
    #[must_use]
    pub fn short_id(&self) -> &str {
        self.id.get(..SHORT_ID_LEN).unwrap_or(&self.id)
    }

    /// Verifies the body against its checksum again (a listed record already carries the result).
    ///
    /// # Errors
    ///
    /// A [`LexDataError::CorruptBook`] naming the file when the body is damaged.
    pub fn verify_body(&self) -> Result<(), LexDataError> {
        book_store::verify_file(&self.path)
    }
}

/// Which books to list: every one when all fields are `None`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BookFilter {
    /// Only this language's books.
    pub lang: Option<Language>,
    /// Only this word length.
    pub word_length: Option<usize>,
    /// Only books whose key names this Profile (a book with an unreadable key never matches).
    pub profile: Option<Profile>,
}

/// What a run would use now, looked up once per Corpus.
struct Current<'a> {
    /// The data directory holding the Corpus caches.
    dir: &'a DataDir,
    /// The solver digest compiled into this build.
    solver_digest: &'a str,
    /// Cached Corpus checksums by (language, length); `None` when no Corpus is cached.
    corpora: HashMap<(Language, usize), Option<String>>,
}

impl<'a> Current<'a> {
    /// A lookup against `dir` and this build's `solver_digest`.
    fn new(dir: &'a DataDir, solver_digest: &'a str) -> Self {
        Self {
            dir,
            solver_digest,
            corpora: HashMap::new(),
        }
    }

    /// The cached Corpus checksum, `None` when the Corpus is absent or unreadable.
    fn corpus(&mut self, lang: Language, n: usize) -> Option<&str> {
        self.corpora
            .entry((lang, n))
            .or_insert_with(|| {
                self.dir
                    .ngrams_path(lang, n)
                    .is_file()
                    .then(|| self.dir.ngrams_sha256(lang, n).ok())
                    .flatten()
            })
            .as_deref()
    }

    /// The stale fields of a book under `key`. A Corpus that is not cached cannot be compared,
    /// so it never makes a book stale.
    fn stale_fields(&mut self, lang: Language, key: &BookKey) -> Vec<StaleField> {
        let mut out = Vec::new();
        let mut push = |name: &str, book: &str, current: &str| {
            if book != current {
                out.push(StaleField {
                    name: name.to_owned(),
                    book: book.to_owned(),
                    current: current.to_owned(),
                });
            }
        };
        if let Some(current) = self.corpus(lang, key.word_length) {
            push("corpus_sha256", &key.corpus_sha256, current);
        }
        if let Some(profile) = &key.profile {
            match Profile::VARIANTS
                .iter()
                .find(|p| p.to_string() == profile.name)
            {
                Some(known) => {
                    let spec = known.spec();
                    push(
                        "profile_answers_sha256",
                        &profile.answers_sha256,
                        spec.answers.sha256,
                    );
                    push(
                        "profile_guess_only_sha256",
                        &profile.guess_only_sha256,
                        spec.guess_only.sha256,
                    );
                }
                None => push("profile", &profile.name, "not a built-in Profile"),
            }
        }
        push("solver_digest", &key.solver_digest, self.solver_digest);
        out
    }

    /// The record of the book file at `path` in the `lang`/`n` directory.
    fn record(&mut self, lang: Language, n: usize, path: &Path) -> BookRecord {
        let id = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let size_bytes = std::fs::metadata(path).map_or(0, |m| m.len());
        let mut record = BookRecord {
            language: lang,
            word_length: n,
            path: path.to_path_buf(),
            size_bytes,
            id,
            header: None,
            status: BookStatus::Current,
        };
        let header = match book_store::read_header(path) {
            Ok(header) => header,
            Err(e) => {
                record.status = BookStatus::Corrupt(reason(&e));
                return record;
            }
        };
        let key = header.key;
        record.header = Some(BookHeader {
            format_version: header.version,
            positions: header.node_count,
            key: key.clone(),
        });
        record.status = if let Err(e) = book_store::verify_file(path) {
            BookStatus::Corrupt(reason(&e.into()))
        } else if key.hash() != record.id {
            BookStatus::Corrupt("its key does not match its file name".to_owned())
        } else if key.word_length != n {
            BookStatus::Corrupt("its word length disagrees with its directory".to_owned())
        } else {
            let mut stale = self.stale_fields(lang, &key);
            let current = book_store::format_version();
            if header.version != current {
                stale.push(StaleField {
                    name: "format_version".to_owned(),
                    book: header.version.to_string(),
                    current: current.to_string(),
                });
            }
            match stale {
                stale if stale.is_empty() => BookStatus::Current,
                stale => BookStatus::Stale(stale),
            }
        };
        record
    }
}

/// The reason inside a corrupt-book error, else the error's own chain.
fn reason(error: &anyhow::Error) -> String {
    match error.downcast_ref::<LexDataError>() {
        Some(LexDataError::CorruptBook { reason, .. }) => reason.clone(),
        _ => format!("{error:#}"),
    }
}

/// The subdirectories of `dir` named by a number, ascending by that number.
fn length_dirs(dir: &Path) -> Vec<(usize, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(usize, PathBuf)> = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|e| Some((e.file_name().to_str()?.parse().ok()?, e.path())))
        .collect();
    out.sort();
    out
}

impl DataDir {
    /// Every stored book matching `filter`, in language order, then word length, then file name.
    /// Reads and verifies every file; nothing is built. `solver_digest` is the current build's, which this
    /// crate cannot know. A damaged book lists as corrupt.
    #[must_use]
    pub fn books(&self, solver_digest: &str, filter: &BookFilter) -> Vec<BookRecord> {
        let mut current = Current::new(self, solver_digest);
        let profile = filter.profile.map(|p| p.to_string());
        let mut records = Vec::new();
        for &lang in Language::VARIANTS {
            if filter.lang.is_some_and(|l| l != lang) {
                continue;
            }
            let lang_dir = self.books_root().join(lang.lang_code());
            for (n, dir) in length_dirs(&lang_dir) {
                if filter.word_length.is_some_and(|w| w != n) {
                    continue;
                }
                for path in book_store::book_files(&dir) {
                    let record = current.record(lang, n, &path);
                    let wanted = profile.as_ref().is_none_or(|name| {
                        record
                            .header
                            .as_ref()
                            .and_then(|h| h.key.profile.as_ref())
                            .is_some_and(|p| &p.name == name)
                    });
                    if wanted {
                        records.push(record);
                    }
                }
            }
        }
        records
    }

    /// The record of the book file at `path`, which sits in the `lang` directory at length `n`.
    #[must_use]
    pub fn book_record(
        &self,
        lang: Language,
        n: usize,
        path: &Path,
        solver_digest: &str,
    ) -> BookRecord {
        Current::new(self, solver_digest).record(lang, n, path)
    }

    /// Removes the books [`DataDir::books`] marks stale or corrupt under `filter`, returning each
    /// file removed.
    ///
    /// # Errors
    ///
    /// If a book file cannot be removed.
    pub fn clear_stale_books(
        &self,
        solver_digest: &str,
        filter: &BookFilter,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let mut removed = Vec::new();
        for record in self.books(solver_digest, filter) {
            if record.status == BookStatus::Current {
                continue;
            }
            std::fs::remove_file(&record.path)
                .with_context(|| format!("removing {}", record.path.display()))?;
            removed.push(record.path);
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book_store::{ProfileKey, StoredBook, StoredNode};
    use crate::header::Header;

    const LANG: Language = Language::English;
    const DIGEST: &str = "now";

    fn dir() -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    fn corpus_key(corpus: &str, digest: &str) -> BookKey {
        BookKey {
            corpus_sha256: corpus.to_owned(),
            profile: None,
            word_length: 5,
            dictionary_length: 10,
            strategy: vec![("pool".to_owned(), "all".to_owned())],
            solver_digest: digest.to_owned(),
        }
    }

    fn book() -> StoredBook {
        StoredBook {
            nodes: vec![StoredNode {
                guess: 1,
                technique: 0,
                guesses: vec![],
                edges: vec![],
            }],
        }
    }

    /// Rewrites the format version in the header of the book file at `path`.
    fn set_version(path: &Path, version: u16) {
        let mut bytes = std::fs::read(path).unwrap();
        bytes[4..6].copy_from_slice(&version.to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn an_older_format_version_is_stale_on_the_format_and_cleared_as_stale() {
        let (_tmp, dir) = dir();
        let key = corpus_key("any", DIGEST);
        let file = dir.write_book(LANG, &key, &book()).unwrap();
        set_version(&file.path, 1);
        let record = &all(&dir)[0];
        assert_eq!(record.header.as_ref().unwrap().format_version, 1);
        assert_eq!(
            record.status,
            BookStatus::Stale(vec![StaleField {
                name: "format_version".to_owned(),
                book: "1".to_owned(),
                current: book_store::format_version().to_string(),
            }])
        );
        assert_eq!(
            dir.clear_stale_books(DIGEST, &BookFilter::default())
                .unwrap(),
            [file.path]
        );
    }

    #[test]
    fn a_newer_or_unknown_format_version_stays_corrupt() {
        for version in [book_store::format_version() + 1, 0] {
            let (_tmp, dir) = dir();
            let file = dir
                .write_book(LANG, &corpus_key("any", DIGEST), &book())
                .unwrap();
            set_version(&file.path, version);
            assert!(
                matches!(all(&dir)[0].status, BookStatus::Corrupt(_)),
                "{version}"
            );
        }
    }

    /// Writes a Corpus cache of one record and returns the checksum its header records.
    fn write_corpus(dir: &DataDir, n: usize) -> String {
        let path = dir.ngrams_path(LANG, n);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let header = Header::for_records(n, 28, &[7u8; 28]).unwrap();
        std::fs::write(&path, header.encode().unwrap()).unwrap();
        dir.ngrams_sha256(LANG, n).unwrap()
    }

    fn all(dir: &DataDir) -> Vec<BookRecord> {
        dir.books(DIGEST, &BookFilter::default())
    }

    #[test]
    fn nothing_stored_lists_nothing() {
        let (_tmp, dir) = dir();
        assert!(all(&dir).is_empty());
    }

    #[test]
    fn a_book_built_under_the_current_state_is_current() {
        let (_tmp, dir) = dir();
        let corpus = write_corpus(&dir, 5);
        let key = corpus_key(&corpus, DIGEST);
        let file = dir.write_book(LANG, &key, &book()).unwrap();
        let records = all(&dir);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.status, BookStatus::Current);
        assert_eq!(record.path, file.path);
        assert_eq!(record.size_bytes, file.bytes);
        assert_eq!(record.id, key.hash());
        assert!(key.hash().starts_with(record.short_id()));
        assert_eq!(record.short_id().len(), 8);
        let header = record.header.as_ref().unwrap();
        assert_eq!(header.positions, 1);
        assert_eq!(header.key, key);
        record.verify_body().unwrap();
    }

    #[test]
    fn an_old_solver_digest_or_a_changed_corpus_is_stale_with_both_values() {
        let (_tmp, dir) = dir();
        let corpus = write_corpus(&dir, 5);
        dir.write_book(LANG, &corpus_key(&corpus, "then"), &book())
            .unwrap();
        dir.write_book(LANG, &corpus_key("other", DIGEST), &book())
            .unwrap();
        let mut statuses: Vec<_> = all(&dir).into_iter().map(|r| r.status).collect();
        statuses.sort_by_key(|s| format!("{s:?}"));
        let field = |name: &str, book: &str, current: &str| StaleField {
            name: name.to_owned(),
            book: book.to_owned(),
            current: current.to_owned(),
        };
        assert_eq!(
            statuses,
            [
                BookStatus::Stale(vec![field("corpus_sha256", "other", &corpus)]),
                BookStatus::Stale(vec![field("solver_digest", "then", DIGEST)]),
            ]
        );
    }

    #[test]
    fn an_uncached_corpus_is_not_stale() {
        let (_tmp, dir) = dir();
        dir.write_book(LANG, &corpus_key("any", DIGEST), &book())
            .unwrap();
        assert_eq!(all(&dir)[0].status, BookStatus::Current);
    }

    #[test]
    fn a_profile_book_is_stale_when_its_list_checksums_differ_from_the_built_in_ones() {
        let (_tmp, dir) = dir();
        let corpus = write_corpus(&dir, 5);
        let spec = Profile::Wordle.spec();
        let mut key = corpus_key(&corpus, DIGEST);
        key.profile = Some(ProfileKey {
            name: "wordle".to_owned(),
            answers_sha256: "old".to_owned(),
            guess_only_sha256: spec.guess_only.sha256.to_owned(),
        });
        dir.write_book(LANG, &key, &book()).unwrap();
        assert_eq!(
            all(&dir)[0].status,
            BookStatus::Stale(vec![StaleField {
                name: "profile_answers_sha256".to_owned(),
                book: "old".to_owned(),
                current: spec.answers.sha256.to_owned(),
            }])
        );
    }

    #[test]
    fn a_different_strategy_is_a_different_book_not_a_stale_one() {
        let (_tmp, dir) = dir();
        let mut key = corpus_key("any", DIGEST);
        key.strategy = vec![("pool".to_owned(), "hard".to_owned())];
        dir.write_book(LANG, &key, &book()).unwrap();
        assert_eq!(all(&dir)[0].status, BookStatus::Current);
    }

    #[test]
    fn a_damaged_header_or_a_wrong_name_is_corrupt() {
        let (_tmp, dir) = dir();
        let file = dir
            .write_book(LANG, &corpus_key("any", DIGEST), &book())
            .unwrap();
        let renamed = file.path.with_file_name("0123456789abcdef.bin");
        std::fs::copy(&file.path, &renamed).unwrap();
        let junk = file.path.with_file_name("junk.bin");
        std::fs::write(&junk, b"not a book").unwrap();
        let records = all(&dir);
        let corrupt: Vec<_> = records
            .iter()
            .filter(|r| matches!(r.status, BookStatus::Corrupt(_)))
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(corrupt, ["0123456789abcdef", "junk"]);
        assert!(records.iter().any(|r| r.status == BookStatus::Current));
    }

    #[test]
    fn a_flipped_body_byte_lists_as_corrupt_and_stale_clearing_removes_it() {
        let (_tmp, dir) = dir();
        let file = dir
            .write_book(LANG, &corpus_key("any", DIGEST), &book())
            .unwrap();
        let mut bytes = std::fs::read(&file.path).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        std::fs::write(&file.path, bytes).unwrap();
        let records = all(&dir);
        assert!(matches!(records[0].status, BookStatus::Corrupt(_)));
        assert!(records[0].verify_body().is_err());
        let removed = dir
            .clear_stale_books(DIGEST, &BookFilter::default())
            .unwrap();
        assert_eq!(removed, [file.path]);
    }

    #[test]
    fn filters_narrow_by_language_length_and_profile() {
        let (_tmp, dir) = dir();
        dir.write_book(LANG, &corpus_key("a", DIGEST), &book())
            .unwrap();
        let mut six = corpus_key("b", DIGEST);
        six.word_length = 6;
        dir.write_book(Language::French, &six, &book()).unwrap();
        let mut wordle = corpus_key("c", DIGEST);
        wordle.profile = Some(ProfileKey {
            name: "wordle".to_owned(),
            answers_sha256: String::new(),
            guess_only_sha256: String::new(),
        });
        dir.write_book(LANG, &wordle, &book()).unwrap();
        let count = |filter: BookFilter| dir.books(DIGEST, &filter).len();
        assert_eq!(count(BookFilter::default()), 3);
        let english = BookFilter {
            lang: Some(LANG),
            ..BookFilter::default()
        };
        assert_eq!(count(english), 2);
        let six = BookFilter {
            word_length: Some(6),
            ..BookFilter::default()
        };
        assert_eq!(count(six), 1);
        let wordle = BookFilter {
            profile: Some(Profile::Wordle),
            ..BookFilter::default()
        };
        assert_eq!(count(wordle), 1);
        let wordle_all = BookFilter {
            profile: Some(Profile::WordleAll),
            ..BookFilter::default()
        };
        assert_eq!(count(wordle_all), 0);
    }

    #[test]
    fn clearing_stale_removes_exactly_the_stale_and_corrupt_files() {
        let (_tmp, dir) = dir();
        let current = dir
            .write_book(LANG, &corpus_key("a", DIGEST), &book())
            .unwrap();
        let stale = dir
            .write_book(LANG, &corpus_key("a", "then"), &book())
            .unwrap();
        let junk = current.path.with_file_name("junk.bin");
        std::fs::write(&junk, b"junk").unwrap();
        let mut removed = dir
            .clear_stale_books(DIGEST, &BookFilter::default())
            .unwrap();
        removed.sort();
        let mut expected = vec![stale.path, junk];
        expected.sort();
        assert_eq!(removed, expected);
        assert!(current.path.exists());
        assert!(
            dir.clear_stale_books(DIGEST, &BookFilter::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_nearest_book_names_its_differing_fields() {
        let (_tmp, dir) = dir();
        dir.write_book(LANG, &corpus_key("a", "then"), &book())
            .unwrap();
        let (path, fields) = dir.nearest_book(LANG, &corpus_key("a", DIGEST)).unwrap();
        assert_eq!(fields, ["solver_digest"]);
        assert!(path.exists());
        assert!(
            dir.nearest_book(Language::French, &corpus_key("a", DIGEST))
                .is_none()
        );
    }
}
