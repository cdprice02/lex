//! Pure handling of one Profile word list: normalize, validate, canonicalize.

use std::collections::HashSet;

use sha2::{Digest, Sha256};

use crate::error::LexDataError;
use crate::parse::normalize;
use crate::store::hex;
use crate::word::Word;

fn invalid(file: &str, line: usize, reason: impl Into<String>) -> anyhow::Error {
    LexDataError::InvalidProfileList {
        file: file.to_string(),
        line,
        reason: reason.into(),
    }
    .into()
}

/// Normalizes each line of `text` in input order. Empty lines are allowed only at the end of the
/// file (the final newline included); one anywhere else is an error.
pub(crate) fn parse(file: &str, text: &str) -> anyhow::Result<Vec<String>> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    while lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let word = normalize(line);
            if word.is_empty() {
                return Err(invalid(
                    file,
                    i + 1,
                    "empty line before the end of the file",
                ));
            }
            Ok(word)
        })
        .collect()
}

/// Checks every word against length `N`, the Alphabet and uniqueness. Line numbers are positions
/// in `words`, which is the file's line order.
pub(crate) fn validate<const N: usize>(
    file: &str,
    words: &[String],
    alphabet: &HashSet<char>,
) -> anyhow::Result<Vec<Word<N>>> {
    let mut seen = HashSet::with_capacity(words.len());
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let line = i + 1;
            let len = w.chars().count();
            if len != N {
                return Err(invalid(
                    file,
                    line,
                    format!("{w:?} has {len} letters, expected {N}"),
                ));
            }
            if let Some(c) = w.chars().find(|c| !alphabet.contains(c)) {
                return Err(invalid(
                    file,
                    line,
                    format!("{w:?} has {c:?}, which is not in the Corpus Alphabet"),
                ));
            }
            if !seen.insert(w.as_str()) {
                return Err(invalid(file, line, format!("{w:?} is a duplicate")));
            }
            Ok(Word::try_from(w.as_str())?)
        })
        .collect()
}

/// A line-numbered complaint for callers that find a violation across lists.
pub(crate) fn complain(file: &str, line: usize, reason: impl Into<String>) -> anyhow::Error {
    invalid(file, line, reason)
}

/// The stored form of a list: sorted, `\n` after every word. Its SHA-256 is the checksum
/// compiled into the registry.
pub(crate) struct Canonical {
    pub(crate) text: String,
    pub(crate) sha256: String,
}

pub(crate) fn canonicalize(mut words: Vec<String>) -> Canonical {
    words.sort_unstable();
    let mut text = String::with_capacity(words.iter().map(|w| w.len() + 1).sum());
    for w in &words {
        text.push_str(w);
        text.push('\n');
    }
    let sha256 = checksum(text.as_bytes());
    Canonical { text, sha256 }
}

/// Hex SHA-256 of a stored list's bytes.
pub(crate) fn checksum(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alphabet(s: &str) -> HashSet<char> {
        s.chars().collect()
    }

    fn words(ws: &[&str]) -> Vec<String> {
        ws.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn parse_normalizes_and_allows_trailing_blank_lines() {
        assert_eq!(
            parse("f", "Crane\nSLATE\n\n\n").unwrap(),
            ["crane", "slate"]
        );
        assert_eq!(parse("f", "crane").unwrap(), ["crane"]);
        assert!(parse("f", "").unwrap().is_empty());
    }

    #[test]
    fn parse_rejects_interior_blank_line_with_its_number() {
        let e = parse("answers.txt", "crane\n\nslate\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("answers.txt") && e.contains("line 2"), "{e}");
    }

    #[test]
    fn validate_names_file_and_line_for_wrong_length() {
        let e = validate::<5>("g.txt", &words(&["crane", "sla"]), &alphabet("cranesl"))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("g.txt") && e.contains("line 2") && e.contains("expected 5"),
            "{e}"
        );
    }

    #[test]
    fn validate_rejects_out_of_alphabet_character() {
        let e = validate::<5>("g.txt", &words(&["crane", "qlate"]), &alphabet("cranesl"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("line 2") && e.contains("Alphabet"), "{e}");
    }

    #[test]
    fn validate_rejects_duplicates_at_the_second_occurrence() {
        let e = validate::<5>(
            "g.txt",
            &words(&["crane", "slate", "crane"]),
            &alphabet("cranesltd"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("line 3") && e.contains("duplicate"), "{e}");
    }

    #[test]
    fn canonical_form_is_sorted_and_newline_terminated() {
        let c = canonicalize(words(&["slate", "crane"]));
        assert_eq!(c.text, "crane\nslate\n");
        assert_eq!(
            c.sha256,
            hex(&Sha256::digest(b"crane\nslate\n")),
            "checksum is over the stored bytes"
        );
    }

    #[test]
    fn empty_list_has_the_empty_checksum() {
        assert_eq!(
            canonicalize(vec![]).sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
