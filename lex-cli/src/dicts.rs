//! `lex dict list` and `lex dict show`: the cached Dictionaries as a `tabled` table on a
//! terminal and as tab-separated columns with one header line when piped. Reads disk only.

use std::fmt::Write as _;
use std::io::IsTerminal as _;

use lex_data::{DataDir, DictRecord, Language};
use tabled::builder::Builder;
use tabled::settings::Style;

use crate::cli::{DictListArgs, DictShowArgs};
use crate::style;

/// What a Dictionary without Provenance shows where Provenance would be.
const UNKNOWN: &str = "unknown";

/// `lex dict list`: a row per cached Dictionary, or "no Dictionaries cached".
///
/// # Errors
///
/// If a cached file cannot be read or parsed.
pub fn list(args: &DictListArgs) -> anyhow::Result<()> {
    let mut records = DataDir::new(&args.data.data_dir).dicts()?;
    records.retain(|r| args.lang.is_none_or(|lang| r.language == lang));
    print_rendered(&render_list(&records, std::io::stdout().is_terminal()));
    Ok(())
}

/// `lex dict show --lang L`: everything known about one cached Dictionary.
///
/// # Errors
///
/// If the Dictionary is not cached, or a cached file cannot be read or parsed.
pub fn show(args: &DictShowArgs) -> anyhow::Result<()> {
    let record = DataDir::new(&args.data.data_dir)
        .dict(args.lang)?
        .ok_or_else(|| not_cached(args.lang))?;
    print_rendered(&render_show(&record, std::io::stdout().is_terminal()));
    Ok(())
}

/// The error for a language whose Dictionary is not on disk.
fn not_cached(lang: Language) -> anyhow::Error {
    anyhow::anyhow!(not_cached_hint(lang))
}

/// What `dict show` says of a language whose Dictionary is not on disk.
#[must_use]
pub fn not_cached_hint(lang: Language) -> String {
    format!("no {lang} Dictionary is cached; fetch it with `lex dict fetch --lang {lang}`")
}

/// Prints through `anstream`, which strips escapes off a terminal or under `NO_COLOR`.
fn print_rendered(text: &str) {
    anstream::print!("{text}");
}

/// The list as a table (`terminal`) or tab-separated lines.
#[must_use]
pub fn render_list(records: &[DictRecord], terminal: bool) -> String {
    if records.is_empty() {
        return "no Dictionaries cached\n".to_owned();
    }
    let header = [
        "language", "code", "words", "fetched", "checksum", "size", "corpus",
    ];
    let rows: Vec<Vec<String>> = records
        .iter()
        .map(|r| {
            let meta = r.metadata.as_ref();
            vec![
                r.language.to_string(),
                r.language.lang_code().to_owned(),
                r.word_count.to_string(),
                meta.and_then(|m| m.fetched_at())
                    .map_or_else(|| UNKNOWN.to_owned(), fetch_date),
                meta.and_then(|m| m.source_sha256())
                    .map_or_else(|| UNKNOWN.to_owned(), |s| s.chars().take(8).collect()),
                size(r.size_bytes, terminal),
                lengths(&r.corpus_lengths),
            ]
        })
        .collect();
    render_rows(&header, rows, terminal)
}

/// One Dictionary in full as a table (`terminal`) or tab-separated lines.
#[must_use]
pub fn render_show(record: &DictRecord, terminal: bool) -> String {
    let meta = record.metadata.as_ref();
    let unknown = |value: Option<&str>| value.map_or_else(|| UNKNOWN.to_owned(), str::to_owned);
    let mut rows = vec![
        row("language", record.language.to_string(), ""),
        row("code", record.language.lang_code(), ""),
        row("source url", unknown(meta.and_then(|m| m.source_url())), ""),
        row("fetched at", unknown(meta.and_then(|m| m.fetched_at())), ""),
        row(
            "source sha256",
            unknown(meta.and_then(|m| m.source_sha256())),
            "",
        ),
        row("word list", record.path.display().to_string(), ""),
        row(
            "metadata",
            record.meta_path.display().to_string(),
            if meta.is_none() { "missing" } else { "" },
        ),
        row("size", size(record.size_bytes, terminal), ""),
        row("words", record.word_count.to_string(), ""),
    ];
    if let Some(meta) = meta {
        let note = record.metadata_mismatch().map_or_else(String::new, |_| {
            flag(
                &format!("MISMATCH: the file holds {}", record.word_count),
                terminal,
            )
        });
        rows.push(row("words in metadata", meta.word_count.to_string(), note));
    }
    for &(n, count) in &record.length_counts {
        let cached = record.corpus_lengths.contains(&n);
        rows.push(row(
            format!("words of length {n}"),
            count.to_string(),
            if cached { "corpus cached" } else { "" },
        ));
    }
    rows.push(row(
        format!(
            "words outside {}..={}",
            lex_data::MIN_WORD_LENGTH,
            lex_data::MAX_WORD_LENGTH
        ),
        record.outside_supported.to_string(),
        "",
    ));
    render_rows(&["field", "value", "note"], rows, terminal)
}

/// A show row of field, value and note.
#[must_use]
pub fn row(
    field: impl Into<String>,
    value: impl Into<String>,
    note: impl Into<String>,
) -> Vec<String> {
    vec![field.into(), value.into(), note.into()]
}

/// A note in the failure style on a terminal.
#[must_use]
pub fn flag(text: &str, terminal: bool) -> String {
    if terminal {
        let style = style::failure_style();
        format!("{style}{text}{style:#}")
    } else {
        text.to_owned()
    }
}

/// `rows` under `header`: a rounded table on a terminal, else tab-separated with one header line.
#[must_use]
pub fn render_rows(header: &[&str], rows: Vec<Vec<String>>, terminal: bool) -> String {
    if terminal {
        let mut builder = Builder::new();
        builder.push_record(header.iter().map(|h| (*h).to_owned()));
        for row in rows {
            builder.push_record(row);
        }
        let mut table = builder.build();
        table.with(Style::rounded());
        return format!("{table}\n");
    }
    let mut out = String::new();
    let _ = writeln!(out, "{}", header.join("\t"));
    for row in rows {
        let _ = writeln!(out, "{}", row.join("\t"));
    }
    out
}

/// The date of an RFC 3339 timestamp.
#[must_use]
pub fn fetch_date(timestamp: &str) -> String {
    timestamp.split('T').next().unwrap_or(timestamp).to_owned()
}

/// Corpus lengths as `5,6,7`, or `-` for none.
fn lengths(lengths: &[usize]) -> String {
    if lengths.is_empty() {
        return "-".to_owned();
    }
    lengths
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// A byte size: MiB with one decimal on a terminal, bytes when piped.
#[must_use]
pub fn size(bytes: u64, terminal: bool) -> String {
    if !terminal {
        return bytes.to_string();
    }
    let tenths = bytes.saturating_mul(10).saturating_add(1 << 19) >> 20;
    format!("{}.{} MiB", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp data dir with `files` written under it.
    fn dir_with(files: &[(&str, &str)]) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        for (relative, body) in files {
            let path = tmp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    const META: &str = r#"{"min_word_length":3,"max_word_length":5,"word_count":9,
        "source_url":"https://example.test/x.gz","fetched_at":"2026-05-01T12:34:56Z",
        "source_sha256":"abcdef0123456789ffff"}"#;

    #[test]
    fn piped_list_is_tab_separated_with_one_header_line() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/eng.txt", "cat\ncrane\n"),
            ("dicts/eng.meta.json", META),
            ("ngrams/eng/5.bin", "x"),
            ("ngrams/eng/3.bin", "x"),
            ("dicts/rus.txt", "дом\n"),
        ]);
        let out = render_list(&dir.dicts().unwrap(), false);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(
            lines[0],
            "language\tcode\twords\tfetched\tchecksum\tsize\tcorpus"
        );
        assert_eq!(lines[1], "english\teng\t2\t2026-05-01\tabcdef01\t10\t3,5");
        assert_eq!(lines[2], "russian\trus\t1\tunknown\tunknown\t7\t-");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn an_empty_list_says_so() {
        assert_eq!(render_list(&[], false), "no Dictionaries cached\n");
        assert_eq!(render_list(&[], true), "no Dictionaries cached\n");
    }

    #[test]
    fn terminal_list_is_a_table_with_mib_sizes() {
        let (_tmp, dir) = dir_with(&[("dicts/eng.txt", "cat\n")]);
        let out = render_list(&dir.dicts().unwrap(), true);
        assert!(out.contains('\u{256d}'));
        assert!(out.contains("0.0 MiB"));
    }

    #[test]
    fn size_rounds_to_one_decimal_mib() {
        assert_eq!(size(1_572_864, true), "1.5 MiB");
        assert_eq!(size(123_456_789, true), "117.7 MiB");
        assert_eq!(size(1_572_864, false), "1572864");
    }

    #[test]
    fn show_counts_cyrillic_by_character_and_marks_cached_lengths() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/rus.txt", "дом\nкот\nслово\nа\n"),
            ("ngrams/rus/3.bin", "x"),
        ]);
        let record = dir.dict(Language::Russian).unwrap().unwrap();
        let out = render_show(&record, false);
        assert!(out.starts_with("field\tvalue\tnote\n"));
        assert!(out.contains("\nwords of length 3\t2\tcorpus cached\n"));
        assert!(out.contains("\nwords of length 4\t0\t\n"));
        assert!(out.contains("\nwords of length 5\t1\t\n"));
        assert!(out.contains("\nwords outside 3..=21\t1\t\n"));
        assert!(out.contains("\nsource sha256\tunknown\t\n"));
        assert!(out.contains("\nmetadata\t"));
        assert!(out.contains("\tmissing\n"));
    }

    #[test]
    fn show_prints_full_provenance_and_flags_a_count_mismatch() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/eng.txt", "cat\ndog\n"),
            ("dicts/eng.meta.json", META),
        ]);
        let record = dir.dict(Language::English).unwrap().unwrap();
        let out = render_show(&record, false);
        assert!(out.contains("\nsource url\thttps://example.test/x.gz\t\n"));
        assert!(out.contains("\nfetched at\t2026-05-01T12:34:56Z\t\n"));
        assert!(out.contains("\nsource sha256\tabcdef0123456789ffff\t\n"));
        assert!(out.contains("\nwords\t2\t\n"));
        assert!(out.contains("\nwords in metadata\t9\tMISMATCH: the file holds 2\n"));
    }

    #[test]
    fn matching_counts_carry_no_flag() {
        let (_tmp, dir) = dir_with(&[
            ("dicts/eng.txt", "cat\n"),
            (
                "dicts/eng.meta.json",
                r#"{"min_word_length":3,"max_word_length":3,"word_count":1}"#,
            ),
        ]);
        let record = dir.dict(Language::English).unwrap().unwrap();
        let out = render_show(&record, false);
        assert!(out.contains("\nwords in metadata\t1\t\n"));
        assert!(!out.contains("MISMATCH"));
    }

    #[test]
    fn show_of_an_uncached_language_names_the_fetch_command() {
        let (_tmp, dir) = dir_with(&[]);
        let args = DictShowArgs {
            lang: Language::English,
            data: crate::cli::DataDirArg {
                data_dir: dir.as_ref().to_path_buf(),
            },
        };
        let error = show(&args).unwrap_err().to_string();
        assert!(error.contains("lex dict fetch --lang english"), "{error}");
        assert!(!dir.as_ref().join("dicts").exists(), "nothing was fetched");
    }
}
