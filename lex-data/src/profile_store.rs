//! Fetching, storing and loading a Profile's lists under `profiles/<name>/`.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use anyhow::Context;
use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::LexDataError;
use crate::http;
use crate::list::{self, Canonical};
use crate::profile::{ListSpec, ProfileData, ProfileSpec, ProfileStats};
use crate::word::{Word, WordSet};

pub(crate) const ANSWERS_FILE: &str = "answers.txt";
pub(crate) const GUESS_ONLY_FILE: &str = "guess-only.txt";
pub(crate) const META_FILE: &str = "meta.json";

/// Returns the body of a URL as text. Injected so tests never touch the network.
pub(crate) type Get<'a> = &'a dyn Fn(&str) -> anyhow::Result<String>;

pub(crate) fn http_get(url: &str) -> anyhow::Result<String> {
    http::get(&http::agent(), url, |body| {
        let mut text = String::new();
        body.read_to_string(&mut text)?;
        Ok(text)
    })
}

#[derive(Serialize)]
struct SourceProvenance {
    url: &'static str,
    sha256: &'static str,
}

#[derive(Serialize)]
struct ListProvenance {
    sha256: String,
    words: usize,
    sources: Vec<SourceProvenance>,
}

#[derive(Serialize)]
struct Metadata {
    profile: String,
    fetched_at: String,
    answers: ListProvenance,
    guess_only: ListProvenance,
}

fn provenance(spec: &ListSpec, list: &Canonical) -> ListProvenance {
    ListProvenance {
        sha256: list.sha256.clone(),
        words: list.text.lines().count(),
        sources: spec
            .sources
            .iter()
            .map(|s| SourceProvenance {
                url: s.url,
                sha256: s.sha256,
            })
            .collect(),
    }
}

fn mismatch(file: &str, expected: &str, actual: String) -> anyhow::Error {
    LexDataError::ProfileChecksum {
        file: file.to_string(),
        expected: expected.to_string(),
        actual,
    }
    .into()
}

/// Downloads and merges one list's sources: each is checked against its own checksum and
/// validated, then the merged form against the list's.
fn fetch_list<const N: usize>(
    file: &str,
    spec: &ListSpec,
    alphabet: &HashSet<char>,
    get: Get<'_>,
) -> anyhow::Result<Canonical> {
    let mut merged: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for source in spec.sources {
        let raw = get(source.url)?;
        let words = list::parse(source.url, &raw)?;
        let own = list::canonicalize(words.clone());
        if own.sha256 != source.sha256 {
            return Err(mismatch(source.url, source.sha256, own.sha256));
        }
        list::validate::<N>(source.url, &words, alphabet)?;
        for (i, w) in words.iter().enumerate() {
            if !seen.insert(w.clone()) {
                return Err(list::complain(
                    source.url,
                    i + 1,
                    format!("{w:?} is also in an earlier source"),
                ));
            }
        }
        merged.extend(words);
    }
    let list = list::canonicalize(merged);
    if list.sha256 != spec.sha256 {
        return Err(mismatch(file, spec.sha256, list.sha256));
    }
    Ok(list)
}

/// Writes `text` to `dir/name` through a temp file in `dir` and a rename, so a concurrent reader
/// or writer sees either nothing or the complete file.
fn write_atomic(dir: &Path, name: &str, text: &str) -> anyhow::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.persist(dir.join(name))
        .with_context(|| format!("renaming {name} into place"))?;
    Ok(())
}

/// Fetches both lists and stores them. `meta.json` goes last: it marks the directory complete,
/// and parallel jobs that race here write identical list files.
fn fetch_into<const N: usize>(
    dir: &Path,
    name: &str,
    spec: &ProfileSpec,
    alphabet: &HashSet<char>,
    get: Get<'_>,
) -> anyhow::Result<()> {
    let answers = fetch_list::<N>(ANSWERS_FILE, &spec.answers, alphabet, get)
        .with_context(|| format!("fetching {name} answers"))?;
    let guess_only = fetch_list::<N>(GUESS_ONLY_FILE, &spec.guess_only, alphabet, get)
        .with_context(|| format!("fetching {name} guess-only words"))?;
    std::fs::create_dir_all(dir)?;
    write_atomic(dir, ANSWERS_FILE, &answers.text)?;
    write_atomic(dir, GUESS_ONLY_FILE, &guess_only.text)?;
    let meta = Metadata {
        profile: name.to_string(),
        fetched_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
        answers: provenance(&spec.answers, &answers),
        guess_only: provenance(&spec.guess_only, &guess_only),
    };
    write_atomic(dir, META_FILE, &serde_json::to_string_pretty(&meta)?)?;
    Ok(())
}

/// Reads a stored list, requiring its bytes to hash to `expected`.
fn read_verified(dir: &Path, file: &str, expected: &str) -> anyhow::Result<String> {
    let path = dir.join(file);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {path:?}"))?;
    let actual = list::checksum(&bytes);
    if actual != expected {
        return Err(mismatch(&path.display().to_string(), expected, actual));
    }
    String::from_utf8(bytes).with_context(|| format!("{path:?} is not UTF-8"))
}

fn alphabet_of<const N: usize>(corpus: &WordSet<N>) -> HashSet<char> {
    corpus.words().flat_map(|w| w.into_iter()).collect()
}

/// Loads a Profile against its Corpus, fetching the lists first if `dir` has no `meta.json`.
/// Every load re-hashes the stored files; a mismatch is a hard error and is never refetched.
pub(crate) fn load<const N: usize>(
    dir: &Path,
    name: &str,
    spec: &ProfileSpec,
    corpus: &WordSet<N>,
    get: Get<'_>,
) -> anyhow::Result<ProfileData<N>> {
    if spec.word_length != N {
        return Err(LexDataError::WordLengthError {
            expected: spec.word_length,
            got: N,
        }
        .into());
    }
    let alphabet = alphabet_of(corpus);
    if !dir.join(META_FILE).exists() {
        log::warn!("Fetching Profile {name}...");
        fetch_into::<N>(dir, name, spec, &alphabet, get)?;
    }

    let answers_text = read_verified(dir, ANSWERS_FILE, spec.answers.sha256)?;
    let guess_text = read_verified(dir, GUESS_ONLY_FILE, spec.guess_only.sha256)?;
    let answer_words = list::parse(ANSWERS_FILE, &answers_text)?;
    let guess_words = list::parse(GUESS_ONLY_FILE, &guess_text)?;
    let answers = list::validate::<N>(ANSWERS_FILE, &answer_words, &alphabet)?;
    let guess_only = list::validate::<N>(GUESS_ONLY_FILE, &guess_words, &alphabet)?;

    let answer_set: HashSet<&str> = answer_words.iter().map(String::as_str).collect();
    if let Some((i, w)) = guess_words
        .iter()
        .enumerate()
        .find(|(_, w)| answer_set.contains(w.as_str()))
    {
        return Err(list::complain(
            GUESS_ONLY_FILE,
            i + 1,
            format!("{w:?} is also in {ANSWERS_FILE}"),
        ));
    }

    let floor = corpus
        .word_frequencies()
        .last()
        .map(|(_, f)| f)
        .context("the Corpus is empty, so there is no Unseen-word floor")?;
    let mut wanted: HashSet<Word<N>> = answers.iter().chain(&guess_only).copied().collect();
    let mut pairs: Vec<(Word<N>, u64)> = corpus
        .word_frequencies()
        .filter(|(w, _)| wanted.remove(w))
        .collect();
    let mut floored: Vec<Word<N>> = wanted.into_iter().collect();
    floored.sort_unstable_by_key(|w| **w);
    let floored_count = floored.len();
    pairs.extend(floored.into_iter().map(|w| (w, floor)));

    let pool = WordSet::from_pairs(pairs);
    let stats = ProfileStats {
        name: name.to_string(),
        answers_sha256: spec.answers.sha256.to_string(),
        guess_only_sha256: spec.guess_only.sha256.to_string(),
        answer_count: answers.len(),
        guess_count: pool.len(),
        floor,
        floored: floored_count,
    };
    Ok(ProfileData {
        pool,
        answers,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;
    use crate::profile::Source;

    fn leak<T>(v: T) -> &'static T {
        Box::leak(Box::new(v))
    }

    /// A Profile over three-letter words: answers cat, dog; guess-only: ant, gnu.
    struct Fixture {
        spec: ProfileSpec,
        a_url: &'static str,
        g_url: &'static str,
    }

    fn fixture(answers: &str, guesses: &str) -> Fixture {
        let a = list::canonicalize(list::parse("a", answers).unwrap());
        let g = list::canonicalize(list::parse("g", guesses).unwrap());
        let (a_url, g_url) = ("mem://answers", "mem://guesses");
        let a_src = leak([Source {
            url: a_url,
            sha256: leak(a.sha256.clone()),
        }]);
        let g_src = leak([Source {
            url: g_url,
            sha256: leak(g.sha256.clone()),
        }]);
        Fixture {
            spec: ProfileSpec {
                language: Language::English,
                word_length: 3,
                answers: ListSpec {
                    sources: a_src,
                    sha256: leak(a.sha256),
                },
                guess_only: ListSpec {
                    sources: g_src,
                    sha256: leak(g.sha256),
                },
            },
            a_url,
            g_url,
        }
    }

    fn word(s: &str) -> Word<3> {
        Word::try_from(s).unwrap()
    }

    fn corpus() -> WordSet<3> {
        WordSet::from_pairs(vec![
            (word("cat"), 100),
            (word("dog"), 50),
            (word("emu"), 40),
            (word("ant"), 10),
        ])
    }

    fn getter<'a>(
        f: &'a Fixture,
        a: &'a str,
        g: &'a str,
    ) -> impl Fn(&str) -> anyhow::Result<String> + 'a {
        move |url| {
            if url == f.a_url {
                Ok(a.to_string())
            } else if url == f.g_url {
                Ok(g.to_string())
            } else {
                anyhow::bail!("unexpected url {url}")
            }
        }
    }

    const A: &str = "dog\ncat";
    const G: &str = "gnu\nant\n";

    #[test]
    fn pool_is_corpus_order_then_floored_words_alphabetically_at_the_floor() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        let data = load(tmp.path(), "t", &f.spec, &corpus(), &getter(&f, A, G)).unwrap();

        let pool: Vec<_> = data
            .pool
            .word_frequencies()
            .map(|(w, fr)| (w.to_string(), fr))
            .collect();
        // emu is in the Corpus but not the Profile: dropped. gnu is floored to ant's count.
        assert_eq!(
            pool,
            [
                ("cat".to_string(), 100),
                ("dog".to_string(), 50),
                ("ant".to_string(), 10),
                ("gnu".to_string(), 10),
            ]
        );
        assert_eq!(data.answers, [word("cat"), word("dog")]);
        assert_eq!(data.stats.floor, 10);
        assert_eq!(data.stats.floored, 1);
        assert_eq!(data.stats.answer_count, 2);
        assert_eq!(data.stats.guess_count, 4);
        assert_eq!(data.stats.answers_sha256, f.spec.answers.sha256);
    }

    #[test]
    fn floored_words_sort_after_corpus_words_at_the_floor_count() {
        // aaa is floored to 10 and alphabetically precedes ant, which has 10 in the Corpus
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, "aaa\nant\n");
        let data = load(
            tmp.path(),
            "t",
            &f.spec,
            &corpus(),
            &getter(&f, A, "aaa\nant\n"),
        )
        .unwrap();
        let tail: Vec<_> = data.pool.words().skip(2).map(|w| w.to_string()).collect();
        assert_eq!(tail, ["ant", "aaa"]);
    }

    #[test]
    fn stores_normalized_files_and_meta_then_serves_without_fetching() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        load(tmp.path(), "t", &f.spec, &corpus(), &getter(&f, A, G)).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(ANSWERS_FILE)).unwrap(),
            "cat\ndog\n"
        );
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(tmp.path().join(META_FILE)).unwrap())
                .unwrap();
        assert_eq!(meta["answers"]["sources"][0]["url"], f.a_url);
        assert_eq!(meta["answers"]["words"], 2);

        let offline = |_: &str| -> anyhow::Result<String> { anyhow::bail!("network used") };
        load(tmp.path(), "t", &f.spec, &corpus(), &offline).unwrap();
    }

    #[test]
    fn fetched_content_with_the_wrong_checksum_is_a_hard_error_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        let err = load(
            tmp.path(),
            "t",
            &f.spec,
            &corpus(),
            &getter(&f, "dog\ncat\nemu", G),
        )
        .err()
        .unwrap();
        assert!(format!("{err:#}").contains("checksum mismatch"), "{err:#}");
        assert!(!tmp.path().join(META_FILE).exists());
        assert!(!tmp.path().join(ANSWERS_FILE).exists());
    }

    #[test]
    fn a_tampered_stored_file_is_a_hard_error_on_load() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        load(tmp.path(), "t", &f.spec, &corpus(), &getter(&f, A, G)).unwrap();
        std::fs::write(tmp.path().join(GUESS_ONLY_FILE), "ant\n").unwrap();
        let offline = |_: &str| -> anyhow::Result<String> { anyhow::bail!("network used") };
        let err = load(tmp.path(), "t", &f.spec, &corpus(), &offline)
            .err()
            .unwrap();
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
    }

    #[test]
    fn out_of_alphabet_words_fail_naming_source_and_line() {
        let tmp = tempfile::tempdir().unwrap();
        // 'q' occurs in no Corpus word
        let f = fixture("cat\nqqq\n", G);
        let err = load(
            tmp.path(),
            "t",
            &f.spec,
            &corpus(),
            &getter(&f, "cat\nqqq\n", G),
        )
        .err()
        .unwrap();
        let text = format!("{err:#}");
        assert!(
            text.contains("mem://answers") && text.contains("line 2") && text.contains("Alphabet"),
            "{text}"
        );
    }

    #[test]
    fn a_word_in_both_files_fails_naming_the_guess_only_line() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, "ant\ncat\n");
        let err = load(
            tmp.path(),
            "t",
            &f.spec,
            &corpus(),
            &getter(&f, A, "ant\ncat\n"),
        )
        .err()
        .unwrap();
        let text = err.to_string();
        assert!(
            text.contains(GUESS_ONLY_FILE)
                && text.contains("line 2")
                && text.contains("answers.txt"),
            "{text}"
        );
    }

    #[test]
    fn a_length_other_than_the_profiles_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        let c5: WordSet<5> = WordSet::from_pairs(vec![(Word::try_from("crane").unwrap(), 5)]);
        let err = load::<5>(tmp.path(), "t", &f.spec, &c5, &getter(&f, A, G))
            .err()
            .unwrap();
        assert!(err.to_string().contains("invalid word length"), "{err}");
    }

    #[test]
    fn merged_sources_with_a_shared_word_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        let src = leak([
            Source {
                url: f.a_url,
                sha256: f.spec.answers.sha256,
            },
            Source {
                url: f.a_url,
                sha256: f.spec.answers.sha256,
            },
        ]);
        let mut spec = f.spec;
        spec.answers.sources = src;
        let err = load(tmp.path(), "t", &spec, &corpus(), &getter(&f, A, G))
            .err()
            .unwrap();
        assert!(format!("{err:#}").contains("earlier source"), "{err:#}");
    }

    #[test]
    fn no_sources_stores_an_empty_list() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fixture(A, G);
        let mut spec = f.spec;
        spec.guess_only = crate::profile::Profile::WordleAll.spec().guess_only;
        let data = load(tmp.path(), "t", &spec, &corpus(), &getter(&f, A, G)).unwrap();
        assert_eq!(data.pool.len(), 2);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(GUESS_ONLY_FILE)).unwrap(),
            ""
        );
    }
}
