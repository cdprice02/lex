use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};

use anyhow::Context;

use crate::data_dir::DataDir;
use crate::language::Language;
use crate::ngrams;
use crate::wiktionary;
use crate::word::{Word, WordSet};

pub(crate) const MIN_FREQUENCY: u64 = 1;

/// Builds the ngrams cache for `lang` at word length `N` if it does not already exist.
/// Ensures the Wiktionary dict is present first, then downloads all ngrams shards,
/// filters to Wiktionary-valid words, and writes all length buckets to disk.
pub(crate) async fn build_if_missing<const N: usize>(
    dir: &DataDir,
    lang: Language,
) -> anyhow::Result<()> {
    if !dir.ngrams_path(lang, N).exists() {
        if !dir.dict_path(lang).exists() {
            wiktionary::fetch_dict(lang, dir.as_ref()).await?;
        }
        let valid_words = wiktionary::load_valid_words(lang, dir.as_ref())?;
        log::warn!("Building {} corpus...", lang);
        let by_length = ngrams::fetch(lang).await?;
        let n_buckets = by_length.len();
        let filtered = filter_by_dict(by_length, &valid_words);
        populate(dir, lang, &filtered)?;
        log::info!("Cached {} corpus ({} length buckets)", lang, n_buckets);
    }
    Ok(())
}

/// Reads the cached ngrams CSV for `lang` at word length `N` into a `WordSet`.
pub(crate) fn read<const N: usize>(
    dir: &DataDir,
    lang: Language,
    limit: Option<usize>,
) -> anyhow::Result<WordSet<N>> {
    // TODO: memmap2 zero-allocation path: mmap the CSV, scan for '\n'/',' boundaries,
    // use Word::try_from_ascii_bytes() to fill [char; N] directly.
    let path = dir.ngrams_path(lang, N);
    let f = std::fs::File::open(&path).with_context(|| format!("opening ngrams file: {path:?}"))?;
    let reader = BufReader::new(f);
    let mut frequencies = HashMap::new();
    for line in reader.lines().take(limit.unwrap_or(usize::MAX)) {
        let line = line?;
        let Some((word_str, freq_str)) = line.split_once(',') else {
            continue;
        };
        let Ok(word) = Word::<N>::try_from(word_str) else {
            continue;
        };
        let Ok(freq) = freq_str.parse::<u64>() else {
            continue;
        };
        frequencies.insert(word, freq);
    }
    Ok(WordSet::new(frequencies))
}

/// Removes the cached ngrams file for `lang` at length `n` (Some) or the entire
/// language ngrams directory (None).
pub(crate) fn clear(dir: &DataDir, lang: Language, n: Option<usize>) -> anyhow::Result<()> {
    match n {
        Some(n) => {
            std::fs::remove_file(dir.ngrams_path(lang, n))
                .with_context(|| format!("removing ngrams file for {lang} length {n}"))?;
        }
        None => {
            let lang_dir = dir.as_ref().join("ngrams").join(lang.lang_code());
            std::fs::remove_dir_all(&lang_dir)
                .with_context(|| format!("removing ngrams dir for {lang}"))?;
        }
    }
    Ok(())
}

fn populate(
    dir: &DataDir,
    lang: Language,
    by_length: &HashMap<usize, HashMap<String, u64>>,
) -> anyhow::Result<()> {
    for (&n, words) in by_length {
        write_bucket(dir, lang, n, words)
            .with_context(|| format!("writing ngrams for {lang} length {n}"))?;
    }
    Ok(())
}

fn filter_by_dict(
    by_length: HashMap<usize, HashMap<String, u64>>,
    valid: &HashSet<String>,
) -> HashMap<usize, HashMap<String, u64>> {
    by_length
        .into_iter()
        .filter_map(|(n, words)| {
            let filtered: HashMap<String, u64> = words
                .into_iter()
                .filter(|(w, freq)| valid.contains(w) && *freq >= MIN_FREQUENCY)
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some((n, filtered))
            }
        })
        .collect()
}

fn write_bucket(
    dir: &DataDir,
    lang: Language,
    n: usize,
    words: &HashMap<String, u64>,
) -> anyhow::Result<()> {
    // TODO: fixed-width binary format ([u32; N] chars + u64 freq = N*4+8 bytes/record)
    // once memmap2 is adopted for zero-parse, zero-allocation loading.
    let path = dir.ngrams_path(lang, n);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut entries: Vec<(&str, u64)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    entries.sort_unstable_by_key(|e| std::cmp::Reverse(e.1));
    let mut writer = std::io::BufWriter::new(std::fs::File::create(&path)?);
    for (word, freq) in entries {
        writeln!(writer, "{word},{freq}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::data_dir::DataDir;
    use crate::language::Language;
    use crate::word::{Word, WordSet};

    use super::{clear, filter_by_dict, populate, read};

    fn toy_corpus() -> HashMap<usize, HashMap<String, u64>> {
        let mut five = HashMap::new();
        five.insert("crane".to_string(), 300u64);
        five.insert("stare".to_string(), 200);
        five.insert("light".to_string(), 100);
        five.insert("mount".to_string(), 75);
        five.insert("swipe".to_string(), 25);
        let mut m = HashMap::new();
        m.insert(5, five);
        m
    }

    fn all_words_valid() -> HashSet<String> {
        ["crane", "stare", "light", "mount", "swipe", "ace"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn make_dir_with_dict(words: &HashSet<String>) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        let dict_dir = tmp.path().join("dicts");
        std::fs::create_dir_all(&dict_dir).unwrap();
        let mut sorted: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        sorted.sort_unstable();
        std::fs::write(dir.dict_path(Language::English), sorted.join("\n")).unwrap();
        (tmp, dir)
    }

    #[test]
    fn populate_and_read_roundtrip() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        let filtered = filter_by_dict(toy_corpus(), &valid);
        populate(&dir, Language::English, &filtered).unwrap();

        let ws: WordSet<5> = read::<5>(&dir, Language::English, None).unwrap();
        assert_eq!(ws.len(), 5);
        assert_eq!(
            ws.frequency(&Word::<5>::try_from("crane").unwrap()),
            Some(300)
        );
        assert_eq!(
            ws.frequency(&Word::<5>::try_from("swipe").unwrap()),
            Some(25)
        );
    }

    #[test]
    fn filter_excludes_invalid_words() {
        let valid: HashSet<String> = ["crane", "stare", "light", "mount"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let filtered = filter_by_dict(toy_corpus(), &valid);
        let five = filtered.get(&5).unwrap();
        assert!(!five.contains_key("swipe"));
        assert!(five.contains_key("crane"));
        assert_eq!(five.len(), 4);
    }

    #[test]
    fn filter_drops_empty_buckets() {
        let valid: HashSet<String> = HashSet::new();
        let filtered = filter_by_dict(toy_corpus(), &valid);
        assert!(filtered.is_empty());
    }

    #[test]
    fn read_respects_limit() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        let filtered = filter_by_dict(toy_corpus(), &valid);
        populate(&dir, Language::English, &filtered).unwrap();

        let ws: WordSet<5> = read::<5>(&dir, Language::English, Some(2)).unwrap();
        assert_eq!(ws.len(), 2);
        // written sorted by frequency desc → crane (300), stare (200)
        assert!(ws.contains(&Word::<5>::try_from("crane").unwrap()));
        assert!(ws.contains(&Word::<5>::try_from("stare").unwrap()));
    }

    #[test]
    fn clear_removes_file() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        let filtered = filter_by_dict(toy_corpus(), &valid);
        populate(&dir, Language::English, &filtered).unwrap();

        assert!(dir.ngrams_path(Language::English, 5).exists());
        clear(&dir, Language::English, Some(5)).unwrap();
        assert!(!dir.ngrams_path(Language::English, 5).exists());
    }

    #[test]
    fn clear_removes_language_dir() {
        let valid = all_words_valid();
        let (_tmp, dir) = make_dir_with_dict(&valid);
        let filtered = filter_by_dict(toy_corpus(), &valid);
        populate(&dir, Language::English, &filtered).unwrap();

        let lang_dir = dir.as_ref().join("ngrams").join("eng");
        assert!(lang_dir.exists());
        clear(&dir, Language::English, None).unwrap();
        assert!(!lang_dir.exists());
    }
}
