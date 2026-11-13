//! `lex book list` and `lex book show`: the stored opening books.
//!
//! A `tabled` table on a terminal, tab-separated columns with one header line when piped. Reads
//! disk only: it never builds a book or fetches a Corpus. `show` is the only command that reads
//! more than a book file, and it never fetches either.

use std::io::IsTerminal as _;

use lex_core::{SOLVER_DIGEST, Strategy};
use lex_data::{BookFilter, BookKey, BookRecord, BookStatus};

use crate::books;
use crate::cli::{BookListArgs, BookShowArgs};
use crate::config::{Game, Targets};
use crate::lists::Source;
use crate::table::{flag, render_rows, row};

/// `lex book list`: a row per stored book, then a footer of counts by status and total size.
///
/// # Errors
///
/// Never today; fallible so a later read of the directory can fail.
pub fn list(args: &BookListArgs) -> anyhow::Result<()> {
    let filter = BookFilter {
        lang: args.lang,
        word_length: args.word_length,
        profile: args.profile,
    };
    let mut records = args.data.dir().books(SOLVER_DIGEST, &filter);
    if args.stale {
        records.retain(|r| r.status != BookStatus::Current);
    }
    let terminal = std::io::stdout().is_terminal();
    anstream::print!("{}", render_list(&records, terminal));
    if terminal {
        anstream::println!("{}", footer(&records, terminal));
    } else {
        // Keeps stdout to the header line and one line per book.
        eprintln!("{}", footer(&records, terminal));
    }
    Ok(())
}

/// `lex book show ID`: one stored book, found by the id `list` prints or a unique prefix of it.
///
/// # Errors
///
/// If no book or more than one book has that id.
pub fn show_id(args: &BookShowArgs) -> anyhow::Result<()> {
    let id = args.id.as_deref().unwrap_or_default();
    let dir = args.common.data();
    let mut found: Vec<BookRecord> = dir
        .books(SOLVER_DIGEST, &BookFilter::default())
        .into_iter()
        .filter(|r| !id.is_empty() && r.id.starts_with(id))
        .collect();
    let record = match found.len() {
        0 => anyhow::bail!("no stored opening book has an id starting with {id:?}"),
        1 => found.remove(0),
        n => anyhow::bail!(
            "{n} stored opening books have an id starting with {id:?}; use more characters"
        ),
    };
    print_show(&record, Vec::new());
    Ok(())
}

/// `lex book show` with a game and Strategy: the book that run would read, else the nearest
/// stored book of that language and length with the fields on which it differs.
///
/// # Errors
///
/// If the game's Corpus (or Profile lists) is not cached, or cannot be loaded. Never fetches.
pub fn show_game<const N: usize>(args: &BookShowArgs) -> anyhow::Result<()> {
    let config = args.common.config(Targets::Full);
    let dir = args.common.data();
    let lang = books::language(&config.game);
    if !dir.ngrams_path(lang, N).is_file() {
        anyhow::bail!(
            "no {lang} Corpus of length {N} is cached, so there is no book key to look up"
        );
    }
    if let Game::Profile(profile) = config.game
        && !dir.profile_dir(profile).join("meta.json").is_file()
    {
        anyhow::bail!("the {profile} Profile is not fetched, so there is no book key to look up");
    }
    let (lists, source) = Source::load::<N>(&config, &dir)?;
    let key = books::key(&config, &source, &lists);
    let path = dir.book_path(lang, &key);
    if path.is_file() {
        print_show(&dir.book_record(lang, N, &path, SOLVER_DIGEST), Vec::new());
        return Ok(());
    }
    let Some((nearest, fields)) = dir.nearest_book(lang, &key) else {
        println!(
            "no opening book is stored for this game and Strategy, or for any of this language and length"
        );
        return Ok(());
    };
    let lookup = vec![
        row(
            "lookup",
            "no book for this game and Strategy; the nearest is shown",
            "",
        ),
        row("differs from the requested key in", fields.join(", "), ""),
    ];
    print_show(&dir.book_record(lang, N, &nearest, SOLVER_DIGEST), lookup);
    Ok(())
}

/// Prints one record, verifying its body first.
fn print_show(record: &BookRecord, lookup: Vec<Vec<String>>) {
    let body = record
        .header
        .is_some()
        .then(|| record.verify_body().map_err(|e| e.to_string()));
    anstream::print!(
        "{}",
        render_show(
            record,
            body.as_ref(),
            lookup,
            std::io::stdout().is_terminal()
        )
    );
}

/// The fields of the book's Strategy that differ from `Strategy::DEFAULT` as `name=value`, or `-`.
fn strategy_diff(key: &BookKey) -> String {
    let default = Strategy::DEFAULT.key_fields();
    let differing: Vec<String> = key
        .strategy
        .iter()
        .filter(|(name, value)| {
            default
                .iter()
                .find(|(n, _)| n == name)
                .is_none_or(|(_, v)| v != value)
        })
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    if differing.is_empty() {
        "-".to_owned()
    } else {
        differing.join(",")
    }
}

/// The game a book was built for: the Profile's name, else `language-length`.
fn game(record: &BookRecord) -> String {
    record
        .header
        .as_ref()
        .and_then(|h| h.key.profile.as_ref())
        .map_or_else(
            || format!("{}-{}", record.language, record.word_length),
            |p| p.name.clone(),
        )
}

/// `current`, `stale (field, ...)` or `corrupt`.
fn status(record: &BookRecord) -> String {
    match &record.status {
        BookStatus::Current => "current".to_owned(),
        BookStatus::Stale(fields) => format!(
            "stale ({})",
            fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        BookStatus::Corrupt(_) => "corrupt".to_owned(),
    }
}

/// The list as a table (`terminal`) or tab-separated lines.
#[must_use]
pub fn render_list(records: &[BookRecord], terminal: bool) -> String {
    if records.is_empty() {
        return "no opening books stored\n".to_owned();
    }
    let header = ["id", "game", "strategy", "positions", "size", "status"];
    let rows = records
        .iter()
        .map(|r| {
            vec![
                r.short_id().to_owned(),
                game(r),
                r.header
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |h| strategy_diff(&h.key)),
                r.header
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |h| h.positions.to_string()),
                size(r.size_bytes, terminal),
                flag_if_bad(&status(r), &r.status, terminal),
            ]
        })
        .collect();
    render_rows(&header, rows, terminal)
}

/// Counts by status and the total size of `records`.
#[must_use]
pub fn footer(records: &[BookRecord], terminal: bool) -> String {
    let count = |want: fn(&BookStatus) -> bool| records.iter().filter(|r| want(&r.status)).count();
    let current = count(|s| matches!(s, BookStatus::Current));
    let stale = count(|s| matches!(s, BookStatus::Stale(_)));
    let corrupt = count(|s| matches!(s, BookStatus::Corrupt(_)));
    let total: u64 = records.iter().map(|r| r.size_bytes).sum();
    format!(
        "{} books: {current} current, {stale} stale, {corrupt} corrupt; total size {}",
        records.len(),
        size(total, terminal)
    )
}

/// One book in full as a table (`terminal`) or tab-separated lines. `body` is the outcome of
/// verifying the body checksum, `None` when the header was unreadable; `lookup` rows come first.
#[must_use]
pub fn render_show(
    record: &BookRecord,
    body: Option<&Result<(), String>>,
    lookup: Vec<Vec<String>>,
    terminal: bool,
) -> String {
    let mut rows = lookup;
    rows.push(row("id", &record.id, ""));
    rows.push(row("path", record.path.display().to_string(), ""));
    rows.push(row("size", size(record.size_bytes, terminal), ""));
    rows.push(row("language", record.language.to_string(), ""));
    let status_note = match &record.status {
        BookStatus::Current => String::new(),
        BookStatus::Stale(_) => "a run of this game would not read it".to_owned(),
        BookStatus::Corrupt(reason) => flag(reason, terminal),
    };
    rows.push(row("status", status(record), status_note));
    if let Some(header) = &record.header {
        rows.push(row("format version", header.format_version.to_string(), ""));
        rows.push(row("positions", header.positions.to_string(), ""));
        rows.push(match body {
            Some(Ok(())) => row("body checksum", "verified", ""),
            Some(Err(reason)) => row("body checksum", flag("MISMATCH", terminal), reason),
            None => row("body checksum", "not checked", ""),
        });
        for (name, value) in header.key.fields() {
            let note = match &record.status {
                BookStatus::Stale(fields) => fields
                    .iter()
                    .find(|f| f.name == name)
                    .map_or_else(String::new, |f| {
                        flag(&format!("stale: current is {}", f.current), terminal)
                    }),
                BookStatus::Current | BookStatus::Corrupt(_) => String::new(),
            };
            rows.push(row(name, value, note));
        }
    }
    render_rows(&["field", "value", "note"], rows, terminal)
}

/// `text` in the failure style when `status` is stale or corrupt, on a terminal.
fn flag_if_bad(text: &str, status: &BookStatus, terminal: bool) -> String {
    match status {
        BookStatus::Current => text.to_owned(),
        BookStatus::Stale(_) | BookStatus::Corrupt(_) => flag(text, terminal),
    }
}

/// A byte size: KiB or MiB with one decimal on a terminal, bytes when piped.
fn size(bytes: u64, terminal: bool) -> String {
    if !terminal {
        return bytes.to_string();
    }
    if bytes < 1 << 20 {
        let tenths = bytes.saturating_mul(10).saturating_add(1 << 9) >> 10;
        return format!("{}.{} KiB", tenths / 10, tenths % 10);
    }
    let tenths = bytes.saturating_mul(10).saturating_add(1 << 19) >> 20;
    format!("{}.{} MiB", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_data::DataDir;
    use lex_data::{Language, StoredBook, StoredNode};

    fn key(digest: &str, pool: &str, temperature: &str) -> BookKey {
        BookKey {
            corpus_sha256: "c0".to_owned(),
            profile: None,
            word_length: 5,
            dictionary_length: 10,
            strategy: vec![
                ("pool".to_owned(), pool.to_owned()),
                ("temperature".to_owned(), temperature.to_owned()),
            ],
            solver_digest: digest.to_owned(),
        }
    }

    fn book() -> StoredBook {
        StoredBook {
            nodes: vec![StoredNode {
                guess: 0,
                technique: 0,
                guesses: vec![],
                edges: vec![],
            }],
        }
    }

    fn records(dir: &DataDir) -> Vec<BookRecord> {
        dir.books("now", &BookFilter::default())
    }

    fn temp() -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path());
        (tmp, dir)
    }

    #[test]
    fn strategy_shows_only_fields_that_differ_from_the_default() {
        let default = Strategy::DEFAULT.key_fields();
        let at_default = BookKey {
            strategy: default
                .iter()
                .map(|(n, v)| ((*n).to_owned(), v.clone()))
                .collect(),
            ..key("now", "", "")
        };
        assert_eq!(strategy_diff(&at_default), "-");
        let mut changed = at_default;
        changed.strategy[0].1 = "bogus".to_owned();
        assert_eq!(strategy_diff(&changed), format!("{}=bogus", default[0].0));
    }

    #[test]
    fn piped_list_is_tab_separated_with_one_header_line() {
        let (_tmp, dir) = temp();
        dir.write_book(Language::English, &key("now", "all", "5"), &book())
            .unwrap();
        dir.write_book(Language::English, &key("then", "all", "5"), &book())
            .unwrap();
        std::fs::write(dir.as_ref().join("books/eng/5/junk.bin"), b"not a book").unwrap();
        let out = render_list(&records(&dir), false);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 4, "{out}");
        assert_eq!(lines[0], "id\tgame\tstrategy\tpositions\tsize\tstatus");
        let cells = |i: usize| lines[i].split('\t').collect::<Vec<_>>();
        assert!(lines.iter().any(|l| l.ends_with("\tcorrupt")));
        assert!(lines.iter().any(|l| l.ends_with("\tcurrent")));
        assert!(lines.iter().any(|l| l.ends_with("\tstale (solver_digest)")));
        assert!(cells(1).iter().all(|c| !c.is_empty()));
        assert_eq!(cells(1)[1], "english-5");
        assert_eq!(cells(1)[0].len(), 8);
    }

    #[test]
    fn an_empty_list_says_so_and_the_footer_counts_by_status() {
        assert_eq!(render_list(&[], false), "no opening books stored\n");
        let (_tmp, dir) = temp();
        dir.write_book(Language::English, &key("then", "all", "5"), &book())
            .unwrap();
        let footer = footer(&records(&dir), false);
        assert!(
            footer.starts_with("1 books: 0 current, 1 stale, 0 corrupt; total size "),
            "{footer}"
        );
    }

    #[test]
    fn terminal_list_is_a_table_with_kib_sizes() {
        let (_tmp, dir) = temp();
        dir.write_book(Language::English, &key("now", "all", "5"), &book())
            .unwrap();
        let out = render_list(&records(&dir), true);
        assert!(out.contains('\u{256d}'));
        assert!(out.contains("KiB"));
    }

    #[test]
    fn show_puts_each_stale_value_beside_the_current_one() {
        let (_tmp, dir) = temp();
        dir.write_book(Language::English, &key("then", "all", "5"), &book())
            .unwrap();
        let record = records(&dir).remove(0);
        let out = render_show(&record, Some(&Ok(())), Vec::new(), false);
        assert!(out.starts_with("field\tvalue\tnote\n"));
        assert!(out.contains("\nstatus\tstale (solver_digest)\t"));
        assert!(out.contains("\nsolver_digest\tthen\tstale: current is now\n"));
        assert!(out.contains("\nbody checksum\tverified\t\n"));
        assert!(out.contains("\npositions\t1\t\n"));
        assert!(out.contains("\nformat version\t2\t\n"));
        assert!(out.contains("\nstrategy.pool\tall\t\n"));
    }

    #[test]
    fn show_of_a_corrupt_file_gives_the_reason_and_no_key() {
        let (_tmp, dir) = temp();
        let path = dir.as_ref().join("books/eng/5");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("junk.bin"), b"not a book").unwrap();
        let record = records(&dir).remove(0);
        let out = render_show(&record, None, Vec::new(), false);
        assert!(out.contains("\nstatus\tcorrupt\t"));
        assert!(!out.contains("solver_digest"));
    }

    #[test]
    fn a_damaged_body_is_flagged() {
        let out = {
            let (_tmp, dir) = temp();
            dir.write_book(Language::English, &key("now", "all", "5"), &book())
                .unwrap();
            let record = records(&dir).remove(0);
            render_show(
                &record,
                Some(&Err("body checksum mismatch".to_owned())),
                Vec::new(),
                false,
            )
        };
        assert!(out.contains("\nbody checksum\tMISMATCH\tbody checksum mismatch\n"));
    }

    #[test]
    fn size_rounds_to_one_decimal() {
        assert_eq!(size(1536, true), "1.5 KiB");
        assert_eq!(size(1_572_864, true), "1.5 MiB");
        assert_eq!(size(1536, false), "1536");
    }
}
