use std::fs;
use std::io::Write;

use clap::Parser;
use lex_cli::cli::{Cli, Command};
use lex_cli::report::Report;
use lex_cli::simulate::run;
use lex_cli::tracks::Track;
use lex_data::{DataDir, Language};

/// Writes a cache file in the documented on-disk format, so `load` never touches the network.
fn seed_bin(dir: &DataDir, lang: Language, n: usize, entries: &[(&str, u64)]) {
    use sha2::{Digest, Sha256};
    let path = dir.ngrams_path(lang, n);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut records = Vec::new();
    for &(word, freq) in entries {
        for ch in word.chars() {
            records.extend_from_slice(&(ch as u32).to_le_bytes());
        }
        records.extend_from_slice(&freq.to_le_bytes());
    }
    let mut header = [0u8; 64];
    header[0..4].copy_from_slice(b"LEXC");
    header[4..6].copy_from_slice(&1u16.to_le_bytes());
    header[6..8].copy_from_slice(&(n as u16).to_le_bytes());
    header[8..12].copy_from_slice(&64u32.to_le_bytes());
    header[12..20].copy_from_slice(&(entries.len() as u64).to_le_bytes());
    header[20..52].copy_from_slice(&Sha256::digest(&records));
    let mut f = fs::File::create(path).unwrap();
    f.write_all(&header).unwrap();
    f.write_all(&records).unwrap();
}

/// Stands in for the cached Wiktionary list, so the report's provenance lookup never downloads.
fn seed_dict(dir: &DataDir, lang: Language) {
    let path = dir.dict_path(lang);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "").unwrap();
    fs::write(
        path.with_extension("meta.json"),
        r#"{"min_word_length":5,"max_word_length":5,"word_count":8}"#,
    )
    .unwrap();
}

/// The report as JSON, without the run timestamp, the one field that differs between runs.
fn bytes(mut report: Report) -> String {
    report.run.timestamp.clear();
    serde_json::to_string_pretty(&report).unwrap()
}

#[test]
fn track_flag_and_run_of_its_config_give_equal_reports() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::new(tmp.path());
    seed_bin(
        &dir,
        Language::English,
        5,
        &[
            ("crane", 900),
            ("stare", 800),
            ("light", 700),
            ("mount", 600),
            ("swipe", 500),
            ("slate", 400),
            ("grate", 300),
            ("plate", 200),
        ],
    );

    seed_dict(&dir, Language::English);
    let data_dir = tmp.path().to_str().unwrap();
    let Command::Simulate(args) = Cli::try_parse_from([
        "lex",
        "simulate",
        "--track",
        "english-5",
        "--data-dir",
        data_dir,
    ])
    .unwrap()
    .command
    else {
        unreachable!()
    };

    let track = Track::find("english-5").unwrap();
    let from_flag = run(&args.config(), &dir).unwrap();
    let from_config = run(&track.config(), &dir).unwrap();
    assert_eq!(from_flag.games.len(), 8);
    assert_eq!(bytes(from_flag), bytes(from_config));
}
