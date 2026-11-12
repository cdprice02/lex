//! `lex profile list` and `lex profile show`: the fetched Profiles, tabled or tab-separated.
//!
//! Reads disk only; both commands hash the stored lists against the checksums compiled into
//! this build.

use std::io::IsTerminal as _;

use lex_data::{Profile, ProfileListRecord, ProfileRecord};

use crate::cli::{ProfileListArgs, ProfileShowArgs};
use crate::table::{fetch_date, flag, render_rows, row, yes_no};

/// `lex profile list`: a row per fetched Profile, or "no Profiles fetched".
///
/// # Errors
///
/// If a fetched Profile's files cannot be read or parsed.
pub fn list(args: &ProfileListArgs) -> anyhow::Result<()> {
    let records = args.data.dir().profiles()?;
    anstream::print!("{}", render_list(&records, std::io::stdout().is_terminal()));
    Ok(())
}

/// `lex profile show --profile P`: both lists of one fetched Profile.
///
/// # Errors
///
/// If the Profile is not fetched, or its files cannot be read or parsed.
pub fn show(args: &ProfileShowArgs) -> anyhow::Result<()> {
    let record = args
        .data
        .dir()
        .fetched_profile(args.profile)?
        .ok_or_else(|| not_fetched(args.profile))?;
    anstream::print!("{}", render_show(&record, std::io::stdout().is_terminal()));
    Ok(())
}

/// The error for a Profile whose lists are not on disk.
fn not_fetched(profile: Profile) -> anyhow::Error {
    anyhow::anyhow!(
        "Profile {profile} is not fetched; fetch it with `lex profile fetch --profile {profile}`"
    )
}

/// The list as a table (`terminal`) or tab-separated lines.
#[must_use]
pub fn render_list(records: &[ProfileRecord], terminal: bool) -> String {
    if records.is_empty() {
        return "no Profiles fetched\n".to_owned();
    }
    let header = [
        "profile",
        "language",
        "length",
        "answers",
        "guess-only",
        "fetched",
        "verified",
        "corpus",
    ];
    let rows = records
        .iter()
        .map(|r| {
            vec![
                r.profile.to_string(),
                r.language.to_string(),
                r.word_length.to_string(),
                r.answers.word_count.to_string(),
                r.guess_only.word_count.to_string(),
                fetch_date(&r.fetched_at),
                yes_no(r.verified(), terminal),
                if r.corpus_cached { "cached" } else { "-" }.to_owned(),
            ]
        })
        .collect();
    render_rows(&header, rows, terminal)
}

/// Rows for one list: path, sources, expected and actual checksum.
fn list_rows(name: &str, list: &ProfileListRecord, terminal: bool) -> Vec<Vec<String>> {
    let mut rows = vec![row(
        format!("{name} path"),
        list.path.display().to_string(),
        "",
    )];
    if list.source_urls.is_empty() {
        rows.push(row(
            format!("{name} source url"),
            "none",
            "derived from the other sources",
        ));
    }
    for url in &list.source_urls {
        rows.push(row(format!("{name} source url"), url, ""));
    }
    rows.push(row(
        format!("{name} expected sha256"),
        &list.expected_sha256,
        "compiled in",
    ));
    let (actual, note) = match list.actual_sha256.as_deref() {
        None => (
            "missing",
            flag("MISSING: the file is not on disk", terminal),
        ),
        Some(actual) if list.verified() => (actual, "verified".to_owned()),
        Some(actual) => (actual, flag("MISMATCH", terminal)),
    };
    rows.push(row(format!("{name} actual sha256"), actual, note));
    rows.push(row(
        format!("{name} words"),
        list.word_count.to_string(),
        "",
    ));
    rows
}

/// One Profile in full as a table (`terminal`) or tab-separated lines.
#[must_use]
pub fn render_show(record: &ProfileRecord, terminal: bool) -> String {
    let mut rows = vec![
        row("profile", record.profile.to_string(), ""),
        row("language", record.language.to_string(), ""),
        row("length", record.word_length.to_string(), ""),
        row("directory", record.dir.display().to_string(), ""),
        row("fetched at", &record.fetched_at, ""),
    ];
    rows.extend(list_rows("answers", &record.answers, terminal));
    rows.extend(list_rows("guess-only", &record.guess_only, terminal));
    rows.push(row(
        "corpus",
        if record.corpus_cached {
            "cached"
        } else {
            "not cached"
        },
        "",
    ));
    render_rows(&["field", "value", "note"], rows, terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_data::DataDir;

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

    const META: &str = r#"{"profile":"wordle-all","fetched_at":"2026-05-01T12:34:56Z"}"#;

    #[test]
    fn piped_list_is_tab_separated_with_one_header_line() {
        let (_tmp, dir) = dir_with(&[
            ("profiles/wordle-all/answers.txt", "crane\nslate\n"),
            ("profiles/wordle-all/guess-only.txt", ""),
            ("profiles/wordle-all/meta.json", META),
            ("ngrams/eng/5.bin", "x"),
        ]);
        let out = render_list(&dir.profiles().unwrap(), false);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(
            lines[0],
            "profile\tlanguage\tlength\tanswers\tguess-only\tfetched\tverified\tcorpus"
        );
        assert_eq!(
            lines[1],
            "wordle-all\tenglish\t5\t2\t0\t2026-05-01\tno\tcached"
        );
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn an_empty_list_says_so() {
        assert_eq!(render_list(&[], false), "no Profiles fetched\n");
    }

    #[test]
    fn show_prints_each_list_with_expected_and_actual_checksums() {
        let (_tmp, dir) = dir_with(&[
            ("profiles/wordle-all/guess-only.txt", ""),
            ("profiles/wordle-all/meta.json", META),
        ]);
        let record = dir.fetched_profile(Profile::WordleAll).unwrap().unwrap();
        let out = render_show(&record, false);
        assert!(out.contains("\nfetched at\t2026-05-01T12:34:56Z\t\n"));
        assert!(out.contains("\nanswers source url\thttps://"));
        assert_eq!(out.matches("\nanswers source url\t").count(), 2);
        assert!(out.contains("\nanswers actual sha256\tmissing\tMISSING"));
        assert!(out.contains("\nguess-only source url\tnone\t"));
        assert!(out.contains("\tverified\n"));
        assert!(out.contains("\ncorpus\tnot cached\t\n"));
    }

    #[test]
    fn show_of_an_unfetched_profile_names_the_fetch_command() {
        let (_tmp, dir) = dir_with(&[]);
        let args = ProfileShowArgs {
            profile: Profile::Wordle,
            data: crate::cli::DataDirArg {
                data_dir: dir.as_ref().to_path_buf(),
            },
        };
        let error = show(&args).unwrap_err().to_string();
        assert!(
            error.contains("lex profile fetch --profile wordle"),
            "{error}"
        );
        assert!(
            !dir.as_ref().join("profiles").exists(),
            "nothing was fetched"
        );
    }
}
