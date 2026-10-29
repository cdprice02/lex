//! The Wiktionary dictionary that filters a Corpus down to real words.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

use anyhow::Context;
use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::http;
use crate::language::Language;
use crate::parse::{is_valid_word, normalize};

/// Metadata computed from a language's Wiktionary word list when it is first fetched.
///
/// The provenance fields identify the exact download the word list was derived from. KAIKKI
/// extracts are rolling (the same URL yields different words over time), so two runs are only
/// comparable when their checksums match. They are `None` for caches written before
/// provenance stamping existed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictMetadata {
    /// Shortest word in the list.
    min_word_length: usize,
    /// Longest word in the list.
    max_word_length: usize,
    /// Number of words in the list.
    pub word_count: usize,
    /// Where the extract was downloaded from.
    #[serde(default)]
    source_url: Option<String>,
    /// RFC 3339 time of the download.
    #[serde(default)]
    fetched_at: Option<String>,
    /// SHA-256 of the compressed download.
    #[serde(default)]
    source_sha256: Option<String>,
}

impl DictMetadata {
    /// Metadata for a list of `count` words of lengths `min` to `max`, from the download `provenance` names.
    pub(crate) fn new(min: usize, max: usize, count: usize, provenance: Provenance) -> Self {
        Self {
            min_word_length: min,
            max_word_length: max,
            word_count: count,
            source_url: Some(provenance.source_url),
            fetched_at: Some(provenance.fetched_at),
            source_sha256: Some(provenance.source_sha256),
        }
    }

    /// URL the word list was downloaded from.
    #[must_use]
    pub fn source_url(&self) -> Option<&str> {
        self.source_url.as_deref()
    }

    /// RFC 3339 UTC timestamp of the download.
    #[must_use]
    pub fn fetched_at(&self) -> Option<&str> {
        self.fetched_at.as_deref()
    }

    /// Hex SHA-256 of the compressed bytes as downloaded.
    #[must_use]
    pub fn source_sha256(&self) -> Option<&str> {
        self.source_sha256.as_deref()
    }

    /// The range of word lengths attested in this language's Wiktionary word list.
    #[must_use]
    pub const fn word_length_range(&self) -> std::ops::RangeInclusive<usize> {
        self.min_word_length..=self.max_word_length
    }
}

/// Where a dictionary download came from.
pub struct Provenance {
    /// The URL downloaded.
    source_url: String,
    /// RFC 3339 time of the download.
    fetched_at: String,
    /// SHA-256 of the compressed download.
    source_sha256: String,
}

/// Path of the sorted word list for `lang`.
pub fn dict_txt_path(data_dir: &Path, lang: Language) -> std::path::PathBuf {
    data_dir
        .join("dicts")
        .join(format!("{}.txt", lang.lang_code()))
}

/// Path of the metadata for `lang`.
fn dict_meta_path(data_dir: &Path, lang: Language) -> std::path::PathBuf {
    data_dir
        .join("dicts")
        .join(format!("{}.meta.json", lang.lang_code()))
}

/// Downloads the KAIKKI Wiktionary extract for `lang`, extracts all valid headwords
/// and inflected forms (excluding proper nouns), and writes the sorted word list and
/// metadata to `data_dir/dicts/`.
pub fn fetch_dict(lang: Language, data_dir: &Path) -> anyhow::Result<()> {
    let iso = lang.iso_code();
    let url = lang.wiktionary_url();
    log::info!("Fetching Wiktionary dict for {lang} from {url}...");

    let (words, source_sha256) = http::get(&http::agent(), &url, |body| {
        let mut hashing = HashingReader {
            inner: body,
            hasher: Sha256::new(),
        };
        let words = read_dict(BufReader::new(MultiGzDecoder::new(&mut hashing)), iso)?;
        Ok((words, crate::store::hex(&hashing.hasher.finalize())))
    })
    .with_context(|| format!("fetching Wiktionary for {lang}"))?;
    let provenance = Provenance {
        source_url: url,
        fetched_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
        source_sha256,
    };

    let min_len = words.iter().map(|w| w.chars().count()).min().unwrap_or(0);
    let max_len = words.iter().map(|w| w.chars().count()).max().unwrap_or(0);
    let word_count = words.len();

    let txt_path = dict_txt_path(data_dir, lang);
    std::fs::create_dir_all(txt_path.parent().unwrap())?;
    let mut sorted: Vec<&str> = words.iter().map(std::string::String::as_str).collect();
    sorted.sort_unstable();
    let mut writer = BufWriter::new(std::fs::File::create(&txt_path)?);
    for word in sorted {
        writeln!(writer, "{word}")?;
    }

    let meta = DictMetadata::new(min_len, max_len, word_count, provenance);
    std::fs::write(
        dict_meta_path(data_dir, lang),
        serde_json::to_string_pretty(&meta)?,
    )?;

    log::info!("Cached {lang} Wiktionary dict ({word_count} words, lengths {min_len}–{max_len})");
    Ok(())
}

/// Hashes the compressed bytes as they stream past, so the provenance checksum identifies the
/// download rather than the decoded text.
struct HashingReader<R> {
    /// The wrapped reader.
    inner: R,
    /// SHA-256 of the bytes read so far.
    hasher: Sha256,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

/// Collects the normalized headwords and inflected forms of every non-proper-noun entry in
/// language `iso` from a decompressed KAIKKI JSONL stream.
fn read_dict(reader: impl BufRead, iso: &str) -> std::io::Result<HashSet<String>> {
    let mut words = HashSet::new();
    for line in reader.lines() {
        let Ok(entry) = serde_json::from_str::<WiktionaryEntry>(&line?) else {
            continue;
        };
        if entry.lang_code != iso {
            continue;
        }
        if entry.pos.as_deref() == Some("name") {
            continue;
        }
        insert_word(&mut words, &entry.word);
        for form in &entry.forms {
            insert_word(&mut words, &form.form);
        }
    }
    Ok(words)
}

/// Reads the cached Wiktionary word list for `lang` into a `HashSet`.
/// Returns an error if the dict file does not exist — call `fetch_dict` first.
pub fn load_valid_words(lang: Language, data_dir: &Path) -> anyhow::Result<HashSet<String>> {
    let path = dict_txt_path(data_dir, lang);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("reading dict for {lang} at {}", path.display()))?;
    Ok(content.lines().map(str::to_owned).collect())
}

/// Reads the per-language metadata written when the dict was last fetched.
pub fn load_metadata(lang: Language, data_dir: &Path) -> anyhow::Result<DictMetadata> {
    let path = dict_meta_path(data_dir, lang);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("reading dict metadata for {lang} at {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parsing metadata for {lang}"))
}

/// Adds `raw` to `set` when it normalizes to a valid word.
fn insert_word(set: &mut HashSet<String>, raw: &str) {
    let normalized = normalize(raw);
    if !normalized.is_empty() && is_valid_word(&normalized) {
        set.insert(normalized);
    }
}

/// One line of the KAIKKI JSONL extract.
#[derive(Deserialize)]
struct WiktionaryEntry {
    /// The headword.
    word: String,
    /// The entry's language code.
    lang_code: String,
    /// Part of speech; proper nouns are excluded.
    pos: Option<String>,
    /// Inflected forms of the headword.
    #[serde(default)]
    forms: Vec<WiktionaryForm>,
}

/// An inflected form of an entry.
#[derive(Deserialize)]
struct WiktionaryForm {
    /// The form's text.
    form: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        r#"{"word":"Crane","lang_code":"en","pos":"noun","forms":[{"form":"cranes"},{"form":"two words"}]}"#,
        "\n",
        r#"{"word":"Paris","lang_code":"en","pos":"name"}"#,
        "\n",
        r#"{"word":"gru","lang_code":"it","pos":"noun"}"#,
        "\n",
        "not json\n",
    );

    #[test]
    fn read_dict_keeps_normalized_words_and_forms_of_one_language() {
        let mut words: Vec<_> = read_dict(FIXTURE.as_bytes(), "en")
            .unwrap()
            .into_iter()
            .collect();
        words.sort();
        assert_eq!(words, ["crane", "cranes"]);
    }

    #[test]
    fn hashing_reader_digests_the_bytes_it_passes_through() {
        let mut reader = HashingReader {
            inner: FIXTURE.as_bytes(),
            hasher: Sha256::new(),
        };
        std::io::copy(&mut reader, &mut std::io::sink()).unwrap();
        assert_eq!(
            crate::store::hex(&reader.hasher.finalize()),
            crate::store::hex(&Sha256::digest(FIXTURE.as_bytes()))
        );
    }
}
