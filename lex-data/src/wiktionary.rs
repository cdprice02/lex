use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_compression::tokio::bufread::GzipDecoder;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use crate::language::Language;
use crate::parse::{is_valid_word, normalize};

// TODO: consider tracking data/dicts/ and data/ngrams/ with git-lfs once
// the generated files are stable enough to commit to the repository.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictMetadata {
    pub min_word_length: usize,
    pub max_word_length: usize,
    pub word_count: usize,
}

pub(crate) fn dict_txt_path(data_dir: &Path, lang: Language) -> PathBuf {
    data_dir
        .join("dicts")
        .join(format!("{}.txt", lang.lang_code()))
}

fn dict_meta_path(data_dir: &Path, lang: Language) -> PathBuf {
    data_dir
        .join("dicts")
        .join(format!("{}.meta.json", lang.lang_code()))
}

/// Downloads the KAIKKI Wiktionary extract for `lang`, extracts all valid headwords
/// and inflected forms, and writes the sorted word list and metadata to `data_dir/dicts/`.
pub async fn fetch_dict(lang: Language, data_dir: &Path) -> anyhow::Result<()> {
    let iso = lang.iso_code();
    let url = format!("https://kaikki.org/dictionary/downloads/{iso}/{iso}-extract.jsonl.gz");
    log::info!("Fetching Wiktionary dict for {} from {url}...", lang);

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("fetching Wiktionary for {lang}"))?;

    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let reader = StreamReader::new(stream);
    let decoder = GzipDecoder::new(BufReader::new(reader));
    let mut lines = BufReader::new(decoder).lines();

    let mut words: HashSet<String> = HashSet::new();

    while let Some(line) = lines.next_line().await? {
        let Ok(entry) = serde_json::from_str::<WiktionaryEntry>(&line) else {
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

    let min_len = words.iter().map(|w| w.chars().count()).min().unwrap_or(0);
    let max_len = words.iter().map(|w| w.chars().count()).max().unwrap_or(0);
    let word_count = words.len();

    let txt_path = dict_txt_path(data_dir, lang);
    std::fs::create_dir_all(txt_path.parent().unwrap())?;
    let mut sorted: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
    sorted.sort_unstable();
    let mut writer = BufWriter::new(std::fs::File::create(&txt_path)?);
    for word in sorted {
        writeln!(writer, "{word}")?;
    }

    let meta = DictMetadata {
        min_word_length: min_len,
        max_word_length: max_len,
        word_count,
    };
    std::fs::write(
        dict_meta_path(data_dir, lang),
        serde_json::to_string_pretty(&meta)?,
    )?;

    log::info!(
        "Cached {} Wiktionary dict ({word_count} words, lengths {min_len}–{max_len})",
        lang
    );
    Ok(())
}

/// Reads the cached word list for `lang` into a HashSet. Returns an error if the
/// dict file does not exist — call `fetch_dict` first to populate it.
pub fn load_valid_words(lang: Language, data_dir: &Path) -> anyhow::Result<HashSet<String>> {
    let path = dict_txt_path(data_dir, lang);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("reading dict for {lang} at {path:?}"))?;
    Ok(content.lines().map(str::to_owned).collect())
}

/// Reads the per-language metadata computed when the dict was last fetched.
pub fn load_metadata(lang: Language, data_dir: &Path) -> anyhow::Result<DictMetadata> {
    let path = dict_meta_path(data_dir, lang);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("reading dict metadata for {lang} at {path:?}"))?;
    serde_json::from_str(&content).with_context(|| format!("parsing metadata for {lang}"))
}

fn insert_word(set: &mut HashSet<String>, raw: &str) {
    let normalized = normalize(raw);
    if !normalized.is_empty() && is_valid_word(&normalized) {
        set.insert(normalized);
    }
}

#[derive(Deserialize)]
struct WiktionaryEntry {
    word: String,
    #[allow(dead_code)]
    lang: String,
    lang_code: String,
    pos: Option<String>,
    #[serde(default)]
    forms: Vec<WiktionaryForm>,
}

#[derive(Deserialize)]
struct WiktionaryForm {
    form: String,
}
