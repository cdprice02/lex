use polars::prelude::*;
use std::io::BufRead;

static DAT_FILE_PATH: &str = "../data/words.dat";
static FREQ_FILE_PATH: &str = "../data/word_frequency.csv";

fn main() -> anyhow::Result<()> {
    let input = std::fs::File::open(DAT_FILE_PATH)?;
    let input = std::io::BufReader::new(input);

    let mut words = Vec::new();
    let mut freqs = Vec::new();
    for line in input.lines() {
        let line = line?;
        if line.starts_with('*') {
            continue;
        }

        assert!(
            line.len() >= 5,
            "all lines at least have a five letter word"
        );
        words.push(String::from(&line[0..5]));

        freqs.push(if line.len() == 5 {
            0
        } else {
            // skip the "commonness" delimiter at index 5
            let rest = if line.len() >= 6 { &line[6..] } else { "" };

            if rest.is_empty() {
                0
            } else {
                rest.split(',')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.parse::<u64>().expect("frequency is a number"))
                    .sum()
            }
        });
    }

    CsvWriter::new(std::fs::File::create(FREQ_FILE_PATH)?)
        .include_header(false)
        .finish(
            &mut df!("word" => words.clone(), "freq" => freqs.clone()).expect("valid dataframe"),
        )?;

    Ok(())
}
