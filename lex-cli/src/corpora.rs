//! `lex corpus list` and `lex corpus show`: the cached Corpora, tabled or tab-separated.
//!
//! Reads disk only; `list` reads headers, `show` also hashes the records.

use std::fmt::Write as _;
use std::io::IsTerminal as _;

use lex_data::{
    CorpusRecord, CorpusStatus, Language, MIN_SUPPORTED_WORDS, NGRAMS_SNAPSHOT, TotalsRecord,
};
use strum::VariantArray as _;

use crate::cli::{CorpusListArgs, CorpusShowArgs};
use crate::table::{flag, render_rows, row, size};

/// What a header-less file shows where its header's fields would be.
const ABSENT: &str = "-";

/// `lex corpus list`: a row per cached Corpus, or "no Corpora cached".
///
/// # Errors
///
/// If a cache file or Dictionary metadata cannot be read.
pub fn list(args: &CorpusListArgs) -> anyhow::Result<()> {
    let dir = args.data.dir();
    let mut records = dir.corpora()?;
    records.retain(|r| {
        args.lang.is_none_or(|lang| r.language == lang)
            && args.word_length.is_none_or(|n| r.word_length == n)
    });
    let mut totals = Vec::new();
    for lang in Language::VARIANTS {
        if args.lang.is_none_or(|wanted| wanted == *lang) {
            totals.extend(dir.totals(*lang)?);
        }
    }
    anstream::print!(
        "{}",
        render_list(&records, &totals, std::io::stdout().is_terminal())
    );
    Ok(())
}

/// `lex corpus show --lang L -l N`: everything known about one cached Corpus.
///
/// # Errors
///
/// If the Corpus is not cached, or a cache file cannot be read.
pub fn show(args: &CorpusShowArgs) -> anyhow::Result<()> {
    let dir = args.data.dir();
    let record = dir
        .corpus(args.lang, args.word_length)?
        .ok_or_else(|| not_cached(args.lang, args.word_length))?;
    let verified = match record.checksum {
        Some(_) => Some(dir.verify_corpus(args.lang, args.word_length)?),
        None => None,
    };
    let totals = dir.totals(args.lang)?;
    anstream::print!(
        "{}",
        render_show(
            &record,
            verified,
            totals.as_ref(),
            std::io::stdout().is_terminal()
        )
    );
    Ok(())
}

/// The error for a Corpus that is not on disk.
fn not_cached(lang: Language, n: usize) -> anyhow::Error {
    anyhow::anyhow!(not_cached_hint(lang, n))
}

/// What `corpus show` says of a Corpus that is not on disk.
#[must_use]
pub fn not_cached_hint(lang: Language, n: usize) -> String {
    format!(
        "no {lang} Corpus of length {n} is cached; build it with `lex corpus build --lang {lang}`"
    )
}

/// A status, in the failure style on a terminal unless it is `ok`.
fn status_cell(status: CorpusStatus, terminal: bool) -> String {
    if status == CorpusStatus::Ok {
        status.label().to_owned()
    } else {
        flag(status.label(), terminal)
    }
}

/// `yes` or `no` for a Supported length, `-` when the header is unreadable.
fn supported_cell(record: &CorpusRecord) -> &'static str {
    match record.supported() {
        Some(true) => "yes",
        Some(false) => "no",
        None => ABSENT,
    }
}

/// What a listing says of a language's Ngrams totals: their size, or `-`.
fn totals_cell(totals: &[TotalsRecord], lang: Language, terminal: bool) -> String {
    totals
        .iter()
        .find(|t| t.language == lang)
        .map_or_else(|| ABSENT.to_owned(), |t| size(t.size_bytes, terminal))
}

/// The list as a table (`terminal`) or tab-separated lines. `totals` are the Ngrams totals
/// cached, whose size shows in each row of their language.
#[must_use]
pub fn render_list(records: &[CorpusRecord], totals: &[TotalsRecord], terminal: bool) -> String {
    if records.is_empty() {
        let mut out = "no Corpora cached\n".to_owned();
        for t in totals {
            let _ = writeln!(
                out,
                "{} Ngrams totals are cached ({})",
                t.language,
                size(t.size_bytes, terminal)
            );
        }
        return out;
    }
    let header = [
        "language",
        "length",
        "words",
        "supported",
        "size",
        "checksum",
        "status",
        "totals",
    ];
    let rows = records
        .iter()
        .map(|r| {
            vec![
                r.language.to_string(),
                r.word_length.to_string(),
                r.word_count
                    .map_or_else(|| ABSENT.to_owned(), |c| c.to_string()),
                supported_cell(r).to_owned(),
                size(r.size_bytes, terminal),
                r.checksum
                    .as_deref()
                    .map_or_else(|| ABSENT.to_owned(), |c| c.chars().take(8).collect()),
                status_cell(r.status, terminal),
                totals_cell(totals, r.language, terminal),
            ]
        })
        .collect();
    render_rows(&header, rows, terminal)
}

/// One Corpus in full as a table (`terminal`) or tab-separated lines. `verified` is whether the
/// records hash to the header's checksum, `None` when there is no readable header to check.
#[must_use]
pub fn render_show(
    record: &CorpusRecord,
    verified: Option<bool>,
    totals: Option<&TotalsRecord>,
    terminal: bool,
) -> String {
    let mut rows = vec![
        row("language", record.language.to_string(), ""),
        row("length", record.word_length.to_string(), ""),
        row("path", record.path.display().to_string(), ""),
        row("size", size(record.size_bytes, terminal), ""),
        row(
            "format version",
            record
                .format_version
                .map_or_else(|| ABSENT.to_owned(), |v| v.to_string()),
            "",
        ),
        row("status", record.status.label(), ""),
        row(
            "words",
            record
                .word_count
                .map_or_else(|| ABSENT.to_owned(), |c| c.to_string()),
            "",
        ),
        row(
            "supported",
            supported_cell(record),
            if record.supported() == Some(false) {
                format!("needs at least {MIN_SUPPORTED_WORDS} words")
            } else {
                String::new()
            },
        ),
        row(
            "checksum",
            record.checksum.as_deref().unwrap_or(ABSENT),
            match verified {
                Some(true) => "verified against the records".to_owned(),
                Some(false) => flag("MISMATCH: the records hash to another value", terminal),
                None => "not checked".to_owned(),
            },
        ),
        row(
            "ngrams snapshot",
            format!("{NGRAMS_SNAPSHOT} (this build)"),
            "",
        ),
        totals.map_or_else(
            || {
                row(
                    "ngrams totals",
                    "not cached",
                    format!("the next download of {} writes them", record.language),
                )
            },
            |t| {
                row(
                    "ngrams totals",
                    format!("cached ({})", size(t.size_bytes, terminal)),
                    format!(
                        "{} words at {} lengths; `lex corpus build --lang {} --force` rebuilds this Corpus from them with no network",
                        t.words, t.lengths, record.language
                    ),
                )
            },
        ),
    ];
    if let Some(fetched) = record.dict_fetched_at.as_deref() {
        rows.push(row("Dictionary fetched at", fetched, ""));
    }
    if record.older_than_dictionary() {
        rows.push(row(
            "warning",
            flag(
                "older than its Dictionary: the Dictionary was fetched after this file was written",
                terminal,
            ),
            format!(
                "rebuild with `lex corpus build --lang {} --force`",
                record.language
            ),
        ));
    }
    render_rows(&["field", "value", "note"], rows, terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_data::DataDir;

    /// A temp data dir with `files` written under it.
    fn dir_with(files: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        for (relative, body) in files {
            let path = tmp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    /// A Cache file of the format this build writes, built by the data layer's own writer.
    fn cache(n: usize, count: usize) -> Vec<u8> {
        let mut bytes = b"LEXC".to_vec();
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(u16::try_from(n).unwrap().to_le_bytes());
        bytes.extend(64u32.to_le_bytes());
        bytes.extend(u64::try_from(count).unwrap().to_le_bytes());
        let records = vec![7u8; (n * 4 + 8) * count];
        bytes.extend(sha256(&records));
        bytes.resize(64, 0);
        bytes.extend(records);
        bytes
    }

    /// SHA-256 through the system `shasum`-free route: the data layer verifies, so tests that
    /// need a valid checksum use the header of a file written by it.
    fn sha256(records: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(records).into()
    }

    #[test]
    fn piped_list_is_tab_separated_with_one_header_line() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/6.bin", cache(6, 2)),
            ("ngrams/eng/5.bin", cache(5, 200)),
            ("ngrams/rus/4.bin", vec![1u8; 10]),
        ]);
        let out = render_list(&dir.corpora().unwrap(), &[], false);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(
            lines[0],
            "language\tlength\twords\tsupported\tsize\tchecksum\tstatus\ttotals"
        );
        let five: Vec<_> = lines[1].split('\t').collect();
        assert_eq!(&five[..4], ["english", "5", "200", "yes"]);
        assert_eq!(five[4], (64 + 28 * 200).to_string());
        assert_eq!(five[5].len(), 8);
        assert_eq!(five[6], "ok");
        assert_eq!(five[7], "-");
        let six: Vec<_> = lines[2].split('\t').collect();
        assert_eq!(&six[..4], ["english", "6", "2", "no"]);
        assert_eq!(lines[3], "russian\t4\t-\t-\t10\t-\tlegacy\t-");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn an_empty_list_says_so() {
        assert_eq!(render_list(&[], &[], false), "no Corpora cached\n");
        assert_eq!(render_list(&[], &[], true), "no Corpora cached\n");
    }

    #[test]
    fn terminal_list_is_a_table_with_mib_sizes() {
        let (_tmp, dir) = dir_with(&[("ngrams/eng/5.bin", cache(5, 1))]);
        let out = render_list(&dir.corpora().unwrap(), &[], true);
        assert!(out.contains('\u{256d}'));
        assert!(out.contains("0.0 MiB"));
    }

    #[test]
    fn show_verifies_and_names_the_snapshot() {
        let (_tmp, dir) = dir_with(&[("ngrams/eng/5.bin", cache(5, 3))]);
        let record = dir.corpus(Language::English, 5).unwrap().unwrap();
        let out = render_show(&record, Some(true), None, false);
        assert!(out.starts_with("field\tvalue\tnote\n"));
        assert!(out.contains("\nformat version\t1\t\n"));
        assert!(out.contains("\nstatus\tok\t\n"));
        assert!(out.contains("\nwords\t3\t\n"));
        assert!(out.contains("\nsupported\tno\tneeds at least 200 words\n"));
        assert!(out.contains("\tverified against the records\n"));
        assert!(out.contains("\nngrams snapshot\t20200217 (this build)\t\n"));
        assert!(!out.contains("warning"));
    }

    #[test]
    fn show_flags_a_mismatch_and_a_legacy_file() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/5.bin", cache(5, 3)),
            ("ngrams/eng/6.bin", vec![1u8; 10]),
        ]);
        let five = dir.corpus(Language::English, 5).unwrap().unwrap();
        assert!(render_show(&five, Some(false), None, false).contains("\tMISMATCH:"));
        let six = dir.corpus(Language::English, 6).unwrap().unwrap();
        let out = render_show(&six, None, None, false);
        assert!(out.contains("\nstatus\tlegacy\t\n"));
        assert!(out.contains("\nchecksum\t-\tnot checked\n"));
    }

    #[test]
    fn a_stale_corpus_warns_and_suggests_a_rebuild() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/5.bin", cache(5, 1)),
            ("dicts/eng.txt", b"crane\n".to_vec()),
            (
                "dicts/eng.meta.json",
                br#"{"min_word_length":5,"max_word_length":5,"word_count":1,"fetched_at":"2999-01-01T00:00:00Z"}"#.to_vec(),
            ),
        ]);
        let record = dir.corpus(Language::English, 5).unwrap().unwrap();
        let list = render_list(std::slice::from_ref(&record), &[], false);
        assert!(list.contains("\tolder than its Dictionary\t-\n"), "{list}");
        let out = render_show(&record, Some(true), None, false);
        assert!(out.contains("\nDictionary fetched at\t2999-01-01T00:00:00Z\t\n"));
        assert!(
            out.contains("lex corpus build --lang english --force"),
            "{out}"
        );
    }

    #[test]
    fn show_of_an_uncached_corpus_names_the_build_command() {
        let (_tmp, dir) = dir_with(&[]);
        let args = CorpusShowArgs {
            lang: Language::English,
            word_length: 5,
            data: crate::cli::DataDirArg {
                data_dir: dir.as_ref().to_path_buf(),
            },
        };
        let error = show(&args).unwrap_err().to_string();
        assert!(error.contains("lex corpus build --lang english"), "{error}");
        assert!(!dir.as_ref().join("ngrams").exists(), "nothing was built");
    }

    fn totals_record(lang: Language, size_bytes: u64) -> TotalsRecord {
        TotalsRecord {
            language: lang,
            path: std::path::PathBuf::from("totals"),
            size_bytes,
            lengths: 3,
            words: 42,
            shard_count: 24,
            snapshot: NGRAMS_SNAPSHOT.to_owned(),
        }
    }

    #[test]
    fn the_list_shows_the_totals_size_on_the_rows_of_their_language() {
        let (_tmp, dir) = dir_with(&[
            ("ngrams/eng/5.bin", cache(5, 3)),
            ("ngrams/fre/5.bin", cache(5, 3)),
        ]);
        let totals = [totals_record(Language::English, 1234)];
        let out = render_list(&dir.corpora().unwrap(), &totals, false);
        let lines: Vec<_> = out.lines().collect();
        assert!(lines[1].starts_with("english\t5\t") && lines[1].ends_with("\t1234"));
        assert!(lines[2].starts_with("french\t5\t") && lines[2].ends_with("\t-"));
    }

    #[test]
    fn totals_alone_are_said_when_no_corpus_is_cached() {
        let totals = [totals_record(Language::Russian, 2048)];
        assert_eq!(
            render_list(&[], &totals, false),
            "no Corpora cached\nrussian Ngrams totals are cached (2048)\n"
        );
    }

    #[test]
    fn show_says_whether_the_totals_are_cached() {
        let (_tmp, dir) = dir_with(&[("ngrams/eng/5.bin", cache(5, 3))]);
        let record = dir.corpus(Language::English, 5).unwrap().unwrap();
        let cached = totals_record(Language::English, 4096);
        let out = render_show(&record, Some(true), Some(&cached), false);
        assert!(out.contains("\nngrams totals\tcached (4096)\t42 words at 3 lengths;"));
        let out = render_show(&record, Some(true), None, false);
        assert!(out.contains("\nngrams totals\tnot cached\t"));
    }
}
