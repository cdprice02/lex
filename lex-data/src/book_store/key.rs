//! The identity of an Opening book: the [`BookKey`] and the Profile it names.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store::hex;

/// Domain tag hashed ahead of the key fields.
const KEY_HASH_DOMAIN: &str = "lex-book-key-v1";
/// A Profile as a book key names it: the stored name, plus the checksums of both lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileKey {
    /// The Profile's stored name.
    pub name: String,
    /// SHA-256 of the stored Answer list.
    pub answers_sha256: String,
    /// SHA-256 of the stored guess-only list.
    pub guess_only_sha256: String,
}

/// Everything that can change a guess the solver chooses.
///
/// The Strategy arrives as ordered (field name, value) strings rendered by `lex-core`, which
/// this crate stores and compares without knowing what the fields mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookKey {
    /// The Corpus checksum.
    pub corpus_sha256: String,
    /// Absent on a Corpus run.
    pub profile: Option<ProfileKey>,
    /// The word length.
    pub word_length: usize,
    /// Size of the Guess pool the book indexes into.
    pub dictionary_length: usize,
    /// The Strategy as ordered (field name, value) strings.
    pub strategy: Vec<(String, String)>,
    /// The hash of the solver source the book was built with.
    pub solver_digest: String,
}

impl BookKey {
    /// The key as ordered (name, value) pairs; the unit a stale book is diagnosed in.
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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
